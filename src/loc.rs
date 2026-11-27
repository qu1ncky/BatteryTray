//! The app's texts in Russian, English and Chinese: the windows', the providers', the tray's and the updater's.
//! The windows ask for theirs by key through `t_in`. The language follows the system unless chosen in the settings.
//! On macOS the texts that speak of Windows have their own wording (MAC), and the Swift app's own texts are here too.

use std::fmt::Display;
use std::sync::atomic::{AtomicU8, Ordering};

pub const LANGUAGES: [&str; 3] = ["ru", "en", "zh"];

static CURRENT: AtomicU8 = AtomicU8::new(1);

/// The language for a setting: "ru", "en" or "zh" as chosen, anything else follows the system.
pub fn resolve(setting: Option<&str>) -> &'static str {
    match setting.and_then(|s| LANGUAGES.iter().find(|&&l| l == s)) {
        Some(l) => l,
        None => crate::sys::ui_language(),
    }
}

pub fn set(language: &str) {
    let index = LANGUAGES.iter().position(|&l| l == language).unwrap_or(1);
    CURRENT.store(index as u8, Ordering::Relaxed);
}

pub fn current() -> &'static str {
    LANGUAGES[CURRENT.load(Ordering::Relaxed) as usize]
}

/// A key's texts: on macOS its own wording first.
fn entry(key: &str) -> Option<&'static (&'static str, &'static str, &'static str, &'static str)> {
    #[cfg(target_os = "macos")]
    if let Some(found) = MAC.iter().find(|s| s.0 == key) {
        return Some(found);
    }
    STRINGS.iter().find(|s| s.0 == key)
}

/// Every key with its text in a language, for a window that takes them all at once (the macOS app).
pub fn all_in(language: &str) -> Vec<(&'static str, String)> {
    #[cfg(target_os = "macos")]
    let mac = MAC.iter().map(|s| s.0);
    #[cfg(not(target_os = "macos"))]
    let mac = std::iter::empty();
    let mut keys: Vec<&'static str> = mac.chain(STRINGS.iter().map(|s| s.0)).collect();
    keys.dedup();
    keys.sort_unstable();
    keys.dedup();
    keys.into_iter().map(|k| (k, t_in(language, k))).collect()
}

/// The text for a key in the current language; anything that isn't a key comes back as it is.
pub fn t(key: &str) -> String {
    match entry(key) {
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
    match entry(key) {
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
    ("theme.header", "Тема", "Theme", "主题"),
    ("theme.auto", "Как в системе", "System", "跟随系统"),
    ("theme.light", "Светлая", "Light", "浅色"),
    ("theme.dark", "Тёмная", "Dark", "深色"),
    // Next to a section that is new in this version; taken away in the version after it
    ("settings.new", "новое", "New", "新"),
    ("tray.style", "Отображение заряда", "Battery level display", "电量显示方式"),
    ("tray.style.icon", "Значок батареи", "Battery icon", "电池图标"),
    ("tray.style.number", "Только число", "Number only", "纯数字"),
    ("tray.priority", "Приоритет в трее", "Tray device priority", "托盘优先显示"),
    ("tray.priority.mouse", "Сначала мышь", "Mouse first", "鼠标优先"),
    ("tray.priority.keyboard", "Сначала клавиатура", "Keyboard first", "键盘优先"),
    ("tray.priority.bluetooth", "Сначала Bluetooth", "Bluetooth first", "蓝牙设备优先"),
    ("tray.priority.headset", "Сначала наушники", "Headset first", "耳机优先"),
    ("tray.priority.lowest", "По наименьшему заряду", "Lowest battery first", "最低电量优先"),
    ("hide.offline", "Скрывать неактивные устройства", "Hide inactive devices", "隐藏不活跃设备"),
    ("alerts.header", "Напоминания", "Reminders", "提醒"),
    ("alerts.low", "Напоминать, когда заряд ниже", "Remind me when the battery is below", "电量低于此值时提醒"),
    ("alerts.critical", "Критический уровень", "Critical level", "严重低电量"),
    ("alerts.critical.hint", "На этом уровне напоминание появится снова, даже если вы его отложили",
        "At this level the reminder comes back even if you snoozed it", "达到此电量时，即使已推迟也会再次提醒"),
    ("alerts.critical.info", "Что такое критический уровень", "What the critical level is", "什么是严重低电量"),
    ("alerts.repeat", "Повторять напоминание каждые", "Repeat the reminder every", "重复提醒间隔"),
    ("alerts.repeat.unit", "мин", "min", "分钟"),
    ("alerts.autoclose", "Автоматически закрывать напоминание", "Close the reminder automatically", "自动关闭提醒"),
    ("alerts.autoclose.after", "Закрыть через", "Close after", "关闭时间"),
    ("alerts.autoclose.unit", "сек", "s", "秒"),
    ("charged.toggle", "Сообщать, когда зарядка завершена", "Notify when charging is complete", "充电完成时通知"),
    ("charged.hint", "Работает не со всеми устройствами", "Doesn't work with every device", "并非所有设备都支持"),
    ("charged.info", "Для каких устройств это работает", "Which devices this works with", "适用于哪些设备"),
    ("alerts.test", "Показать пример напоминания", "Show a sample reminder", "显示提醒示例"),
    // The sounds: what each is for, what the lists offer (sounds.rs), what the settings say about a file
    ("sounds.header", "Звуки", "Sounds", "提示音"),
    ("sounds.low", "Низкий заряд", "Low battery", "低电量"),
    ("sounds.critical", "Критический заряд", "Critical battery", "严重低电量"),
    ("sounds.charged", "Зарядка завершена", "Charging complete", "充电完成"),
    ("sounds.system", "Системный звук", "System sound", "系统提示音"),
    ("sounds.none", "Без звука", "No sound", "无声音"),
    ("sounds.file", "Свой файл…", "Custom file…", "自定义文件…"),
    ("sounds.remove", "Убрать из списка", "Remove from the list", "从列表中移除"),
    ("sounds.reveal", "Показать в Finder", "Show in Finder", "在访达中显示"),
    ("sounds.play", "Прослушать", "Play", "试听"),
    ("sounds.stop", "Остановить", "Stop", "停止"),
    ("sounds.long", "Файл длинный: прозвучат первые 10 секунд", "A long file: its first 10 seconds will play",
        "文件较长：只播放前 10 秒"),
    ("sounds.error", "Не удалось открыть этот файл. Звук остался прежним; подойдёт WAV или MP3 до 10 МБ.",
        "This file couldn't be opened. The sound is as it was; a WAV or MP3 of up to 10 MB will do.",
        "无法打开此文件，提示音保持不变。请选择不超过 10 MB 的 WAV 或 MP3 文件。"),
    // The file dialog: its list of file types, and its button on macOS
    ("sounds.files", "Звуки", "Sounds", "声音文件"),
    ("sounds.choose", "Выбрать", "Choose", "选取"),
    ("updates.header", "Обновления", "Updates", "更新"),
    ("updates.check", "Проверить", "Check now", "立即检查"),
    ("updates.auto", "Обновлять автоматически", "Install updates automatically", "自动安装更新"),
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
    ("alert.snooze", "Напомнить через {0} мин", "Remind me in {0} min", "{0} 分钟后提醒"),
    ("alert.snooze.tip", "Напомнить позже", "Remind me later", "稍后提醒"),
    ("alert.mute", "Не напоминать до зарядки", "Don't remind me until it's charged", "充电前不再提醒"),

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
    ("name.phone", "Телефон", "Phone", "手机"),
    ("name.ups", "ИБП", "UPS", "UPS 电源"),
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
    ("d.wasBefore", "до зарядки {0}%", "{0}% before charging", "充电前为 {0}%"),
    ("d.almostFull", "почти заряжено", "almost full", "即将充满"),
    ("d.unplugOk", "можно отключать кабель", "you can unplug it", "可以拔掉充电线了"),
    ("d.settling", "уточняется после зарядки", "settling after charging", "充电后校准中"),
    ("d.estimate", "примерно, по напряжению", "estimated from voltage", "按电压估算"),
    ("d.onBattery", "работает от батареи", "running on battery", "电池供电中"),
    ("d.busy", "нет доступа: устройство занято другой программой", "no access: another program is using it",
        "无法访问：设备被其他程序占用"),
    ("d.noController", "геймпад не подключён к адаптеру", "no controller on the adapter", "适配器未连接手柄"),
    ("d.batteryError", "геймпад сообщает об ошибке батареи", "the controller reports a battery error", "手柄报告电池故障"),
    ("d.waitFullMode", "заряд появится, когда геймпадом начнёт пользоваться игра или Steam",
        "the level shows once a game or Steam uses the controller", "游戏或 Steam 使用手柄后才会显示电量"),
    ("d.fourLevels", "примерно: геймпад сообщает только 4 уровня", "approximate: the controller reports only 4 levels",
        "约值：手柄仅报告 4 个等级"),
    ("d.fiveLevels", "примерно: геймпад сообщает только 5 уровней", "approximate: the controller reports only 5 levels",
        "约值：手柄仅报告 5 个等级"),
    ("d.tenSteps", "примерно: мышь сообщает заряд шагами по 10%", "approximate: the mouse reports the level in 10% steps",
        "约值：鼠标以 10% 为单位报告电量"),
    ("d.unknownModel", "заряд этой модели программа пока не читает — её поможет добавить лог",
        "this model's level can't be read yet; its log can help add it", "暂时无法读取此型号的电量，日志可帮助添加支持"),
    // The parts of a device with batteries of their own: earbuds and their case, the halves of a split keyboard (the
    // one the computer talks to is the main one; the others are named by the device)
    ("d.budLeft", "левый {0}%", "left {0}%", "左耳 {0}%"),
    ("d.budRight", "правый {0}%", "right {0}%", "右耳 {0}%"),
    ("d.case", "кейс {0}%", "case {0}%", "充电盒 {0}%"),
    ("d.mainPart", "основной {0}%", "main {0}%", "主设备 {0}%"),
];

/// macOS: the texts that speak of Windows, in the words of a Mac, and the texts only the Mac app has.
#[cfg(target_os = "macos")]
static MAC: &[(&str, &str, &str, &str)] = &[
    ("autostart", "Открывать при входе в систему", "Open at login", "登录时打开"),
    ("move.title", "Переместить BatteryTray в «Программы»?", "Move BatteryTray to Applications?",
        "将 BatteryTray 移到“应用程序”文件夹？"),
    ("move.text", "Так программа сможет обновляться сама и открываться при входе в систему.",
        "That way it can update itself and open at login.", "这样它才能自动更新并在登录时打开。"),
    ("move.yes", "Переместить", "Move", "移动"),
    ("move.no", "Не сейчас", "Not now", "暂不"),
    ("move.failed", "Не удалось переместить BatteryTray: {0}", "Couldn't move BatteryTray: {0}", "无法移动 BatteryTray：{0}"),

    ("autostart.banner", "BatteryTray не открывается при входе в систему", "BatteryTray doesn't open at login",
        "BatteryTray 未设置登录时打开"),
    ("feedback.about.mac", "BatteryTray {0} · macOS {1} · язык: русский", "BatteryTray {0} · macOS {1} · language: English",
        "BatteryTray {0} · macOS {1} · 语言：中文"),
    ("overlay.compatibility", "На экране активного приложения, поверх окон и полноэкранных приложений. Не перехватывает мышь и фокус.",
        "Appears on the active app's display, above windows and full-screen apps, without taking focus or mouse input.",
        "显示在当前应用所在的显示器上，位于窗口和全屏应用之上，不抢占焦点或鼠标操作。"),
    ("overlay.invalid", "Выберите другое сочетание. Для обычных клавиш нужен ⌘, ⌃ или ⌥. ⌘Q, ⌘W, ⌘H, ⌘M, ⌘C, ⌘V, ⌘X, ⌘Z, ⌘A, ⌘Tab, ⌘Пробел, ⌃Пробел и снимки экрана (⇧⌘3, 4, 5) зарезервированы.",
        "Choose another shortcut. Ordinary keys require ⌘, ⌃ or ⌥. ⌘Q, ⌘W, ⌘H, ⌘M, ⌘C, ⌘V, ⌘X, ⌘Z, ⌘A, ⌘Tab, ⌘Space, ⌃Space and the screenshot shortcuts (⇧⌘3, 4, 5) are reserved.",
        "请选择其他快捷键。普通按键需搭配 ⌘、⌃ 或 ⌥。⌘Q、⌘W、⌘H、⌘M、⌘C、⌘V、⌘X、⌘Z、⌘A、⌘Tab、⌘空格、⌃空格和截屏快捷键（⇧⌘3、4、5）为保留快捷键。"),
    // The alert for a sound file the app couldn't take: a short title, and what to do in the text under it
    ("sounds.error.title", "Не удалось открыть этот файл.", "This file couldn't be opened.", "无法打开此文件。"),
    ("sounds.error.text", "Звук остался прежним. Подойдёт WAV или MP3 до 10 МБ.",
        "The sound is as it was. A WAV or MP3 of up to 10 MB will do.", "提示音保持不变。请选择不超过 10 MB 的 WAV 或 MP3 文件。"),
    ("key.space", "Пробел", "Space", "空格"),
    ("menu.about", "О программе BatteryTray", "About BatteryTray", "关于 BatteryTray"),
    ("menu.hide", "Скрыть BatteryTray", "Hide BatteryTray", "隐藏 BatteryTray"),
    ("menu.hideOthers", "Скрыть остальные", "Hide Others", "隐藏其他"),
    ("menu.showAll", "Показать все", "Show All", "全部显示"),
    ("menu.edit", "Правка", "Edit", "编辑"),
    ("menu.undo", "Отменить", "Undo", "撤销"),
    ("menu.redo", "Повторить", "Redo", "重做"),
    ("menu.cut", "Вырезать", "Cut", "剪切"),
    ("menu.copy", "Скопировать", "Copy", "拷贝"),
    ("menu.paste", "Вставить", "Paste", "粘贴"),
    ("menu.selectAll", "Выбрать все", "Select All", "全选"),
    ("menu.window", "Окно", "Window", "窗口"),
    ("menu.minimize", "Свернуть", "Minimize", "最小化"),
    ("menu.close", "Закрыть", "Close", "关闭"),
    ("d.needsAdmin", "нужно разрешение «Мониторинг ввода»", "needs the Input Monitoring permission", "需要“输入监控”权限"),
    ("permission.banner", "Для некоторых мышей и клавиатур нужно разрешение «Мониторинг ввода»",
        "Some mice and keyboards need the Input Monitoring permission", "部分鼠标和键盘需要“输入监控”权限"),
    ("permission.text", "macOS даёт читать клавиатуры и мыши с клавишами только программам с разрешением «Мониторинг ввода». BatteryTray читает только заряд, нажатия клавиш ей не нужны.",
        "macOS lets only apps with the Input Monitoring permission read keyboards and mice that have keyboard keys. BatteryTray reads only the battery level, never what you type.",
        "macOS 仅允许拥有“输入监控”权限的应用读取键盘和带键盘按键的鼠标。BatteryTray 只读取电量，从不读取您的输入。"),
    ("permission.allow", "Разрешить", "Allow", "允许"),
    ("permission.settings", "Открыть настройки", "Open Settings", "打开设置"),
    ("menubar.header", "Строка меню", "Menu bar", "菜单栏"),
    ("menubar.devices", "Устройств в строке меню", "Devices in the menu bar", "菜单栏中显示的设备数"),
    ("tray.priority", "Приоритет в строке меню", "Menu bar device priority", "菜单栏优先显示"),
    ("tray.exit", "Завершить BatteryTray", "Quit BatteryTray", "退出 BatteryTray"),
    ("tray.open", "Открыть BatteryTray", "Open BatteryTray", "打开 BatteryTray"),
    ("upd.err.version", "в релизе {0} лежит приложение версии {1}", "release {0} contains an app of version {1}",
        "版本 {0} 中的应用版本为 {1}"),
    ("upd.err.notBundle", "программа запущена не из BatteryTray.app", "the app isn't running from BatteryTray.app",
        "程序不是从 BatteryTray.app 运行的"),
    ("upd.err.translocated", "переместите BatteryTray в папку «Программы», тогда она сможет обновляться",
        "move BatteryTray to the Applications folder so it can update itself", "请将 BatteryTray 移到“应用程序”文件夹，以便它能自动更新"),
    ("upd.err.unpack", "не удалось распаковать архив", "couldn't unpack the archive", "无法解压更新包"),
    ("upd.err.signature", "новая версия подписана не так, как эта, поэтому не установлена",
        "the new version isn't signed as this one is, so it wasn't installed", "新版本的签名与当前版本不一致，因此未安装"),
];
