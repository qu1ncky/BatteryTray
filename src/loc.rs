//! The app's texts in Russian, English and Chinese: the windows', the providers', the tray's and the updater's.
//! The windows ask for theirs by key through `t_in`. The language follows Windows unless chosen in the settings.

use std::fmt::Display;
use std::sync::atomic::{AtomicU8, Ordering};

pub const LANGUAGES: [&str; 3] = ["ru", "en", "zh"];

static CURRENT: AtomicU8 = AtomicU8::new(1);

/// The language for a setting: "ru", "en" or "zh" as chosen, anything else follows Windows.
pub fn resolve(setting: Option<&str>) -> &'static str {
    match setting.and_then(|s| LANGUAGES.iter().find(|&&l| l == s)) {
        Some(l) => l,
        None => crate::win::ui_language(),
    }
}

pub fn set(language: &str) {
    let index = LANGUAGES.iter().position(|&l| l == language).unwrap_or(1);
    CURRENT.store(index as u8, Ordering::Relaxed);
}

pub fn current() -> &'static str {
    LANGUAGES[CURRENT.load(Ordering::Relaxed) as usize]
}

/// The text for a key in the current language; anything that isn't a key comes back as it is.
pub fn t(key: &str) -> String {
    match STRINGS.iter().find(|s| s.0 == key) {
        None => key.to_string(),
        Some(&(_, ru, en, zh)) => match CURRENT.load(Ordering::Relaxed) {
            0 => ru,
            2 => zh,
            _ => en,
        }
        .to_string(),
    }
}

/// The text for a key in a given language (what the windows bind to, so they switch with it).
pub fn t_in(language: &str, key: &str) -> String {
    match STRINGS.iter().find(|s| s.0 == key) {
        None => key.to_string(),
        Some(&(_, ru, en, zh)) => match language {
            "ru" => ru,
            "zh" => zh,
            _ => en,
        }
        .to_string(),
    }
}

/// A day and time the way each language writes it: 18.11 в 15:40, Nov 18, 15:40, 11月18日 15:40.
pub fn date_time(t: &chrono::DateTime<chrono::Local>) -> String {
    use chrono::{Datelike, Timelike};
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let time = format!("{:02}:{:02}", t.hour(), t.minute());
    match current() {
        "ru" => format!("{:02}.{:02} в {time}", t.day(), t.month()),
        "zh" => format!("{}月{}日 {time}", t.month(), t.day()),
        _ => format!("{} {}, {time}", MONTHS[t.month0() as usize], t.day()),
    }
}

/// The text with {0}, {1}... filled in.
pub fn tf(key: &str, args: &[&dyn Display]) -> String {
    let mut text = t(key);
    for (i, arg) in args.iter().enumerate() {
        text = text.replace(&format!("{{{i}}}"), &arg.to_string());
    }
    text
}

static STRINGS: &[(&str, &str, &str, &str)] = &[
    // Window
    ("app.title", "Заряд устройств", "Device Battery", "设备电量"),
    ("settings", "Настройки", "Settings", "设置"),
    ("back", "Назад", "Back", "返回"),
    ("refresh", "Обновить заряд", "Refresh battery levels", "刷新电量"),
    ("autostart.banner", "BatteryTray не запускается вместе с Windows",
        "BatteryTray doesn't start with Windows", "BatteryTray 未设置开机自动启动"),
    ("autostart.banner.enable", "Включить", "Enable", "启用"),
    ("autostart.banner.dismiss", "Больше не показывать", "Don't show again", "不再显示"),
    ("admin.banner", "Windows не даёт прочитать некоторые устройства без прав администратора",
        "Windows won't let the app read some devices without administrator rights",
        "没有管理员权限，Windows 不允许本程序读取某些设备"),
    ("admin.banner.grant", "Дать права", "Grant rights", "授予权限"),
    ("admin.dialog.title", "Дать права администратора", "Grant administrator rights", "授予管理员权限"),
    ("admin.dialog.text", "BatteryTray установится в папку Program Files и будет запускаться вместе с Windows с правами администратора. Windows один раз попросит подтверждение, дальше всё работает само, в том числе обновления.",
        "BatteryTray will install itself into Program Files and start with Windows with administrator rights. Windows asks you to confirm once; after that everything, updates included, works on its own.",
        "BatteryTray 将安装到 Program Files 文件夹，并以管理员权限随 Windows 启动。Windows 只会请求确认一次，之后包括更新在内都会自动进行。"),
    ("admin.dialog.continue", "Продолжить", "Continue", "继续"),
    ("admin.dialog.cancel", "Отмена", "Cancel", "取消"),
    ("none.online", "Сейчас ни одно устройство не на связи", "No devices connected right now", "当前没有已连接的设备"),
    ("offline.header", "Нет связи", "Not connected", "未连接"),
    ("updated.at", "Обновлено в {0}", "Updated at {0}", "更新于 {0}"),
    ("polling", "Опрашиваю устройства…", "Checking devices…", "正在检测设备…"),

    // Device cards
    ("state.charging", "Заряжается", "Charging", "充电中"),
    ("state.full", "Заряжено", "Charged", "已充满"),
    ("state.offline", "Нет связи", "Not connected", "未连接"),
    ("state.online", "На связи", "Connected", "已连接"),
    ("last.seen", "последний раз {0}%, {1}", "last: {0}%, {1}", "上次 {0}%，{1}"),
    ("ago.now", "только что", "just now", "刚刚"),
    ("ago.min", "{0} мин назад", "{0} min ago", "{0} 分钟前"),
    ("ago.hours", "{0} ч назад", "{0} h ago", "{0} 小时前"),
    ("device.name", "Название устройства", "Device name", "设备名称"),
    ("device.pinned", "Закреплено", "Pinned", "已置顶"),
    ("device.drag", "Перетащить, чтобы изменить порядок", "Drag to reorder", "拖动以调整顺序"),
    ("device.menu", "Меню устройства", "Device menu", "设备菜单"),
    ("device.rename", "Переименовать", "Rename", "重命名"),
    ("device.pin", "Закрепить", "Pin", "置顶"),
    ("device.unpin", "Открепить", "Unpin", "取消置顶"),
    ("device.hide", "Скрыть из списка", "Hide from the list", "从列表中隐藏"),
    ("device.alerts", "Напоминать о разряде", "Low battery reminders", "低电量提醒"),
    ("device.low", "Напомнить ниже", "Remind below", "提醒值"),
    ("device.critical", "Критично ниже", "Critical below", "严重值"),
    ("device.levels.reset", "Вернуть общие", "Use the global levels", "恢复全局值"),
    ("hidden.header", "Скрытые", "Hidden", "已隐藏"),
    ("device.show", "Показать", "Show", "显示"),

    // Settings
    ("general.header", "Общие", "General", "常规"),
    ("lang.header", "Язык · Language", "Language", "语言 · Language"),
    ("lang.auto", "Как в системе", "System", "跟随系统"),
    ("alerts.header", "Напоминания", "Reminders", "提醒"),
    ("alerts.low", "Напоминать, когда заряд ниже", "Remind me when the battery is below", "电量低于此值时提醒"),
    ("alerts.critical", "Критический уровень", "Critical level", "严重低电量"),
    ("alerts.critical.hint", "На этом уровне напоминание появится снова, даже если вы его отложили",
        "At this level the reminder comes back even if you snoozed it", "达到此电量时，即使已推迟也会再次提醒"),
    ("alerts.critical.info", "Что такое критический уровень", "What the critical level is", "什么是严重低电量"),
    ("alerts.repeat", "Повторять напоминание каждые", "Repeat the reminder every", "重复提醒间隔"),
    ("alerts.repeat.unit", "мин", "min", "分钟"),
    ("alerts.sound", "Звуковой сигнал", "Sound", "提示音"),
    ("charged.toggle", "Сообщать, когда зарядка завершена", "Notify when charging is complete", "充电完成时通知"),
    ("charged.hint", "Уведомление Windows. Работает с устройствами, которые сами сообщают, что зарядились",
        "A Windows notification, for devices that report a finished charge", "Windows 通知，适用于会报告充电完成的设备"),
    ("charged.info", "Для каких устройств это работает", "Which devices this works with", "适用于哪些设备"),
    ("alerts.test", "Показать пример напоминания", "Show a sample reminder", "显示提醒示例"),
    ("updates.header", "Обновления", "Updates", "更新"),
    ("updates.check", "Проверить", "Check now", "立即检查"),
    ("updates.auto", "Обновлять автоматически", "Install updates automatically", "自动安装更新"),
    ("updates.auto.hint", "Новая версия ставится, пока окна программы закрыты, и программа сама перезапускается.",
        "A new version installs while the app's windows are closed, then the app restarts by itself.",
        "新版本会在程序窗口关闭时安装，之后程序自动重启。"),
    ("updates.auto.info", "Как проходит автоматическое обновление", "How automatic updates work", "自动更新如何进行"),
    ("autostart", "Запускать при входе в Windows", "Start with Windows", "开机自动启动"),
    ("open.folder", "Открыть папку с логом и настройками", "Open the log and settings folder", "打开日志和设置文件夹"),
    ("feedback", "Предложить улучшение", "Suggest an improvement", "提出改进建议"),
    ("feedback.browser", "Откроется в браузере", "Opens in the browser", "将在浏览器中打开"),
    ("feedback.what", "Что предлагаете?", "What do you suggest?", "您有什么建议？"),
    ("feedback.why", "Зачем это нужно?", "Why is it needed?", "为什么需要它？"),
    ("feedback.about", "BatteryTray {0} · Windows {1} (сборка {2}) · язык: русский",
        "BatteryTray {0} · Windows {1} (build {2}) · language: English",
        "BatteryTray {0} · Windows {1}（内部版本 {2}）· 语言：中文"),

    // Overlay
    ("overlay.header", "Оверлей", "Overlay", "悬浮窗"),
    ("overlay.enabled", "Включить горячую клавишу", "Enable overlay shortcut", "启用悬浮窗快捷键"),
    ("overlay.shortcut", "Горячая клавиша", "Shortcut", "快捷键"),
    ("overlay.recording", "Нажмите сочетание…", "Press a shortcut…", "请按下快捷键…"),
    ("overlay.reset", "Сбросить", "Reset", "重置"),
    ("overlay.mode", "Показывать", "Show", "显示时长"),
    ("overlay.timed", "5 секунд", "For 5 seconds", "5 秒"),
    ("overlay.toggle", "До повторного нажатия", "Until pressed again", "直到再次按下"),
    ("overlay.corner", "Угол экрана", "Screen corner", "屏幕角落"),
    ("overlay.top.left", "Сверху слева", "Top left", "左上"),
    ("overlay.top.right", "Сверху справа", "Top right", "右上"),
    ("overlay.bottom.left", "Снизу слева", "Bottom left", "左下"),
    ("overlay.bottom.right", "Снизу справа", "Bottom right", "右下"),
    ("overlay.names", "Всегда показывать названия", "Always show names", "始终显示名称"),
    ("overlay.preview", "Предпросмотр", "Preview", "预览"),
    ("overlay.compatibility", "На экране активного приложения. Не перехватывает мышь и фокус. Работает на рабочем столе и в играх без рамки; эксклюзивный полный экран не поддерживается.",
        "Appears on the active app's monitor without taking focus or mouse input. Supports desktop and borderless games; exclusive fullscreen is not supported.",
        "显示在当前应用所在的屏幕上，不抢占焦点或鼠标操作。支持桌面和无边框游戏；不支持独占全屏。"),
    ("overlay.info", "Где оверлей работает и где нет", "Where the overlay works and where it doesn't", "悬浮窗在哪些情况下可用"),
    ("overlay.unavailable", "Не удалось зарегистрировать горячую клавишу. Выберите другую или выключите и включите её снова.",
        "The shortcut could not be registered. Choose another, or disable and enable it to retry.",
        "无法注册快捷键。请选择其他组合，或关闭后重新启用以重试。"),
    ("overlay.conflict", "Не удалось зарегистрировать сочетание. Предыдущее сохранено; попробуйте другое.",
        "That shortcut is unavailable. The previous shortcut is kept; try another.",
        "该快捷键不可用，已保留原快捷键；请尝试其他组合。"),
    ("overlay.invalid", "Выберите другое сочетание. Для обычных клавиш нужен Ctrl или Alt. Win, F12, Ctrl+C, Ctrl+V, Ctrl+X и Alt+F4 зарезервированы.",
        "Choose another shortcut. Ordinary keys require Ctrl or Alt. Windows keys, F12, Ctrl+C, Ctrl+V, Ctrl+X, and Alt+F4 are reserved.",
        "请选择其他快捷键。普通按键需搭配 Ctrl 或 Alt。Windows 键、F12、Ctrl+C、Ctrl+V、Ctrl+X 和 Alt+F4 为保留快捷键。"),

    // Updates
    ("upd.checking", "Проверяю, нет ли новой версии…", "Checking for a new version…", "正在检查新版本…"),
    ("upd.uptodate", "Установлена последняя версия {0}", "You have the latest version {0}", "已是最新版本 {0}"),
    ("upd.checked", "Проверено в {0}", "Checked at {0}", "检查于 {0}"),
    ("upd.available", "Доступна версия {0}, у вас {1}", "Version {0} is available, you have {1}", "有新版本 {0}，当前版本 {1}"),
    ("upd.downloading", "Загружаю версию {0}: {1}%", "Downloading version {0}: {1}%", "正在下载版本 {0}：{1}%"),
    ("upd.installing", "Устанавливаю версию {0}…", "Installing version {0}…", "正在安装版本 {0}…"),
    ("upd.failed", "Не получилось: {0}", "That didn't work: {0}", "失败：{0}"),
    ("upd.idle", "Версия {0}. Проверка обновлений начнётся через полминуты после запуска.",
        "Version {0}. The update check starts half a minute after launch.", "版本 {0}。启动半分钟后开始检查更新。"),
    ("upd.install", "Обновить до {0}", "Update to {0}", "更新到 {0}"),
    ("upd.banner", "Доступна версия {0}", "Version {0} is available", "有新版本 {0}"),
    ("upd.banner.button", "Обновить", "Update", "更新"),

    // Low battery reminder
    ("alert.title", "Пора зарядить", "Time to charge", "该充电了"),
    ("alert.title.many", "Пора зарядить устройства", "Time to charge your devices", "设备该充电了"),
    ("alert.title.critical", "Почти разрядилось!", "Almost empty!", "电量即将耗尽！"),
    ("alert.snooze", "Напомнить через {0} мин", "Remind me in {0} min", "{0} 分钟后提醒"),
    ("alert.snooze.tip", "Напомнить позже", "Remind me later", "稍后提醒"),
    ("alert.mute", "Не напоминать до зарядки", "Don't remind me until it's charged", "充电前不再提醒"),
    ("alert.hint.critical", "Осталось совсем немного — поставьте на зарядку, чтобы не отключилось в самый неподходящий момент.",
        "Very little left: plug it in before it dies at the worst moment.", "电量所剩无几，请及时充电，以免在关键时刻断电。"),
    ("alert.hint", "Окно не закроется само, пока вы не выберете, когда напомнить снова.",
        "This window stays until you choose when to be reminded again.", "在您选择下次提醒时间之前，此窗口不会自动关闭。"),

    // Updates
    ("upd.err.size", "размер файла {0} вместо {1}", "file size {0} instead of {1}", "文件大小为 {0}，应为 {1}"),
    ("upd.err.hash", "контрольная сумма не совпала", "checksum mismatch", "校验和不匹配"),
    ("upd.err.version", "в релизе {0} лежит exe версии {1}", "release {0} contains an exe of version {1}", "版本 {0} 中的程序版本为 {1}"),
    ("upd.err.cancelled", "обновление отменено или не удалось, подробности в логе",
        "the update was cancelled or failed, see the log", "更新已取消或失败，详情见日志"),

    // Tray and notifications
    ("tray.open", "Открыть", "Open", "打开"),
    ("tray.exit", "Выход", "Exit", "退出"),
    ("tray.nodata", "Заряд устройств: нет данных", "Device Battery: no data", "设备电量：暂无数据"),
    ("tray.charging", "{0}: заряжается ⚡", "{0}: charging ⚡", "{0}：充电中 ⚡"),
    ("notice.updated.title", "BatteryTray обновлён", "BatteryTray updated", "BatteryTray 已更新"),
    ("notice.updated.text", "Установлена версия {0}.", "Version {0} is installed.", "已安装版本 {0}。"),
    ("notice.charged.title", "Зарядка завершена", "Charging complete", "充电完成"),
    ("notice.charged.text", "{0} — {1}%. Кабель можно отключать.", "{0} — {1}%. You can unplug it.", "{0} — {1}%。可以拔掉充电线了。"),
    ("notice.available.title", "Доступна версия BatteryTray {0}", "BatteryTray {0} is available", "BatteryTray {0} 可用"),
    ("notice.available.text", "Откройте BatteryTray → Настройки → Общие, чтобы установить.",
        "Open BatteryTray → Settings → General to install it.", "打开 BatteryTray → 设置 → 常规 进行安装。"),

    // What the providers report
    ("via.24", "2.4 ГГц", "2.4 GHz", "2.4 GHz"),
    ("via.cable", "кабель", "cable", "有线"),
    ("via.adapter", "адаптер", "adapter", "适配器"),
    ("via.receiver", "USB-приёмник", "USB receiver", "USB 接收器"),
    ("name.mouse", "Мышь", "Mouse", "鼠标"),
    ("name.keyboard", "Клавиатура", "Keyboard", "键盘"),
    ("name.brandMouse", "Мышь {0}", "{0} mouse", "{0} 鼠标"),
    ("name.brandKeyboard", "Клавиатура {0}", "{0} keyboard", "{0} 键盘"),
    ("name.aresonMouse", "Мышь на приёмнике Areson", "Mouse on an Areson receiver", "Areson 接收器上的鼠标"),
    ("name.fa09Keyboard", "Клавиатура на приёмнике 2.4G", "Keyboard on a 2.4G receiver", "2.4G 接收器上的键盘"),
    ("name.xbox", "Геймпад Xbox", "Xbox controller", "Xbox 手柄"),
    ("d.needsAdmin", "нужны права администратора", "needs administrator rights", "需要管理员权限"),
    ("d.error", "ошибка: {0}", "error: {0}", "错误：{0}"),
    ("d.asleep", "спит или выключено", "asleep or off", "休眠或已关闭"),
    ("d.offOrAway", "выключено или далеко", "off or out of range", "已关闭或超出范围"),
    ("d.receiverSilent", "приёмник не отвечает", "the receiver isn't responding", "接收器无响应"),
    ("d.noLink", "не удалось связаться с устройством", "couldn't reach the device", "无法连接设备"),
    ("d.readFailed", "заряд не прочитался", "couldn't read the battery", "无法读取电量"),
    ("d.levelUnknown", "заряд пока неизвестен", "level not known yet", "暂未获取电量"),
    ("d.cableHidden", "процент будет виден без кабеля", "the level shows once unplugged", "拔掉线后显示电量"),
    ("d.chargerHidden", "процент будет виден, когда мышь снимут с зарядки", "the level shows once it's off the charger",
        "离开充电器后显示电量"),
    ("d.wasBefore", "до зарядки было {0}%", "{0}% before charging", "充电前为 {0}%"),
    ("d.almostFull", "почти заряжено", "almost full", "即将充满"),
    ("d.unplugOk", "можно отключать кабель", "you can unplug it", "可以拔掉充电线了"),
    ("d.settling", "уточняется после зарядки", "settling after charging", "充电后校准中"),
    ("d.estimate", "примерно, по напряжению", "approximate, from the voltage", "按电压估算"),
    ("d.busy", "нет доступа: устройство занято другой программой", "no access: another program is using it",
        "无法访问：设备被其他程序占用"),
    ("d.noController", "геймпад не подключён к адаптеру", "no controller on the adapter", "适配器未连接手柄"),
    ("d.batteryError", "геймпад сообщает об ошибке батареи", "the controller reports a battery error", "手柄报告电池故障"),
    ("d.waitFullMode", "заряд появится, когда геймпадом начнёт пользоваться игра или Steam",
        "the level shows once a game or Steam uses the controller", "游戏或 Steam 使用手柄后才会显示电量"),
    ("d.fourLevels", "примерно: геймпад сообщает только 4 уровня", "approximate: the controller reports only 4 levels",
        "约值：手柄仅报告 4 个等级"),
    ("d.tenSteps", "примерно: мышь сообщает заряд шагами по 10%", "approximate: the mouse reports the level in 10% steps",
        "约值：鼠标以 10% 为单位报告电量"),
    ("d.unknownModel", "заряд этой модели программа пока не читает — её поможет добавить лог",
        "this model's level can't be read yet; its log can help add it", "暂时无法读取此型号的电量，日志可帮助添加支持"),
];
