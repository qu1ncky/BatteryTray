<div align="center">

<img src="docs/icon.png" alt="" width="88">

# BatteryTray

**Заряд беспроводных наушников, мыши, клавиатуры и геймпада —<br>на панели задач Windows и в строке меню Mac.**

[![Последний релиз](https://img.shields.io/github/v/release/qu1ncky/BatteryTray)](https://github.com/qu1ncky/BatteryTray/releases/latest)
[![Загрузки](https://img.shields.io/github/downloads/qu1ncky/BatteryTray/total)](https://github.com/qu1ncky/BatteryTray/releases)
[![Лицензия: MIT](https://img.shields.io/badge/license-MIT-blue)](LICENSE)
![Windows 10 / 11](https://img.shields.io/badge/Windows-10%20%7C%2011-0078D6)
![macOS 13+](https://img.shields.io/badge/macOS-13%2B%20·%20Apple%20Silicon%20%7C%20Intel-000000)

[English](README.md) · **Русский** · [中文](README.zh-CN.md)

<a href="https://github.com/qu1ncky/BatteryTray/releases/latest/download/BatteryTray.exe"><img src="docs/download-windows-ru.svg" alt="Скачать BatteryTray для Windows" height="56"></a>&nbsp;&nbsp;<a href="https://github.com/qu1ncky/BatteryTray/releases/latest/download/BatteryTray-macOS.dmg"><img src="docs/download-macos-ru.svg" alt="Скачать BatteryTray для macOS" height="56"></a>

<sub>Бесплатно и с открытым кодом · без аккаунта и фирменного софта · <a href="#установка">как установить</a> · <a href="https://github.com/qu1ncky/BatteryTray/releases">что нового</a></sub>

<img src="docs/readme/hero-ru.png" alt="BatteryTray в Windows и на Mac: гарнитура Razer Barracuda X — 37%, мышь Attack Shark R3 заряжается, клавиатура Rainy 75 — около 15%" width="720">

</div>

- **Одна программа для всех устройств.** Игровые мыши, клавиатуры и гарнитуры Razer, Logitech, SteelSeries, HyperX, Corsair, ASUS ROG и десятков других брендов — через их приёмники 2,4 ГГц, а ещё AirPods, геймпады Xbox и DualSense и любые Bluetooth-устройства, заряд которых знает система. [Поддерживаемые устройства](docs/ru/devices.md)
- **Всегда на виду.** У часов в Windows и в строке меню на Mac; садящееся устройство выделено тревожным цветом.
- **Напоминание, которое не пропустишь** — до того, как батарея сядет посреди игры или созвона. Режим «Не беспокоить» его не прячет.
- **Поверх игр:** включите оверлей в «Настройки → Оверлей», и <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>V</kbd> (<kbd>⌃⌥V</kbd> на Mac) на пять секунд покажет заряд всех устройств. [Все возможности](docs/ru/features.md)

<p align="center">
  <img src="docs/readme/menubar-ru.gif" alt="Строка меню Mac: гарнитура, мышь и клавиатура с их зарядом; клавиатура садится, становится оранжевой, потом красной и снова заряжается" width="830"><br>
  <sub>Строка меню Mac</sub>
</p>
<p align="center">
  <img src="docs/readme/taskbar-ru.gif" alt="Панель задач Windows: гарнитура, мышь и клавиатура с их зарядом слева от значков у часов; клавиатура садится, становится оранжевой, потом красной и снова заряжается" width="830"><br>
  <sub>Панель задач Windows (Настройки → Общие → Панель задач)</sub>
</p>

<table>
<tr>
<td width="50%"><img src="docs/readme/reminder-ru.png" alt="Напоминание о низком заряде: AirPods Pro — 18%, Razer Barracuda X — 14%, кнопки «Напомнить через 10 мин» и «Не напоминать до зарядки»; полоска внизу показывает, сколько осталось до того, как оно закроется само" width="100%"></td>
<td width="50%"><img src="docs/readme/overlay-ru.png" alt="Оверлей поверх игры: гарнитура — 37%, мышь Attack Shark R3 заряжается, мышь VXE R1 — 82%; показывается по Ctrl+Alt+V" width="100%"></td>
</tr>
<tr>
<td align="center"><sub>Напоминание: отложить, не напоминать до зарядки или дать ему закрыться самому</sub></td>
<td align="center"><sub>Оверлей — поверх игр и полноэкранных приложений</sub></td>
</tr>
</table>

## Установка

<table>
<tr>
<td width="50%" valign="top">

**Windows 10 и 11**

1. Скачайте [`BatteryTray.exe`](https://github.com/qu1ncky/BatteryTray/releases/latest/download/BatteryTray.exe) и запустите.
2. Если Windows предупредит о неизвестном приложении: **Подробнее** → **Выполнить в любом случае**.
3. Заряд появится у часов (Windows 11 может спрятать его под **^** — перетащите на панель задач). Кнопка **«Включить»** над списком устройств добавит программу в автозапуск.

<img src="docs/readme/install-windows-ru.png" alt="Фильтр SmartScreen: нажмите «Подробнее», затем «Выполнить в любом случае»" width="100%">

Или `winget install qu1ncky.BatteryTray`, или [Scoop](https://scoop.sh): `scoop bucket add batterytray https://github.com/qu1ncky/BatteryTray`, затем `scoop install batterytray`.

</td>
<td width="50%" valign="top">

**macOS 13 и новее**, Apple Silicon и Intel

1. Откройте [`BatteryTray-macOS.dmg`](https://github.com/qu1ncky/BatteryTray/releases/latest/download/BatteryTray-macOS.dmg) и перетащите **BatteryTray** в **«Программы»**.
2. Запустите. В первый раз macOS его заблокирует: **Системные настройки → Конфиденциальность и безопасность → Все равно открыть**.
3. Разрешите Bluetooth — заряд появится в строке меню. Кнопка **«Включить»** над списком устройств включит открытие при входе в систему.

<img src="docs/readme/install-macos-ru.png" alt="Системные настройки, «Конфиденциальность и безопасность»: «Файл «BatteryTray» заблокирован для защиты Вашего Mac», кнопка «Все равно открыть»" width="100%">

Разрешения: Bluetooth и «Мониторинг ввода» — только если подключена клавиатура (macOS требует его от любой программы, которая её открывает; BatteryTray читает только заряд). Больше ничего.

</td>
</tr>
</table>

## Подробнее

[Поддерживаемые устройства](docs/ru/devices.md) · [Все возможности](docs/ru/features.md) · [Частые вопросы](docs/ru/faq.md) · [Как это работает](docs/ru/how-it-works.md) · [Сборка из исходников](docs/development.md) · [Что нового](https://github.com/qu1ncky/BatteryTray/releases)

Нет вашего устройства или что-то не так? [Создайте issue](https://github.com/qu1ncky/BatteryTray/issues) и приложите лог: `%LOCALAPPDATA%\BatteryTray\log.txt` в Windows, `~/Library/Application Support/BatteryTray/log.txt` на Mac. Чтобы добавили новое устройство, приложите ещё вывод [этой команды](docs/ru/devices.md#устройства-нет-в-списке), пока оно подключено: она покажет USB ID его приёмника.

## Лицензия

[MIT](LICENSE). Сторонний код, источники протоколов и атрибуция Slint — в [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md). Проект не связан с Apple, Razer, Logitech, SteelSeries, HyperX, Corsair, ASUS, Sony, Microsoft, Valve, ATTACK SHARK и другими упомянутыми производителями; названия продуктов — товарные знаки их владельцев.
