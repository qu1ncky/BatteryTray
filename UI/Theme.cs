using System;
using System.Runtime.InteropServices;
using System.Windows;
using System.Windows.Interop;
using System.Windows.Media;
using BatteryTray.Core;

namespace BatteryTray.UI
{
    /// <summary>Level colors and icon glyphs shared by the windows and the tray icon.</summary>
    public static class Theme
    {
        public static readonly Color GoodColor = Color.FromRgb(0x6C, 0xCB, 0x5F);
        public static readonly Color WarnColor = Color.FromRgb(0xF7, 0xC0, 0x4A);
        public static readonly Color BadColor = Color.FromRgb(0xFF, 0x6B, 0x6B);
        public static readonly Color ChargeColor = Color.FromRgb(0x60, 0xCD, 0xFF);
        public static readonly Color OfflineColor = Color.FromRgb(0x7A, 0x7A, 0x7A);

        public static readonly SolidColorBrush Good = Frozen(GoodColor);
        public static readonly SolidColorBrush Warn = Frozen(WarnColor);
        public static readonly SolidColorBrush Bad = Frozen(BadColor);
        public static readonly SolidColorBrush Charge = Frozen(ChargeColor);
        public static readonly SolidColorBrush Offline = Frozen(OfflineColor);

        static SolidColorBrush Frozen(Color c)
        {
            var b = new SolidColorBrush(c);
            b.Freeze();
            return b;
        }

        public static Color LevelColor(int? level, bool charging, bool online, AppSettings s)
        {
            if (!online) return OfflineColor;
            if (charging) return ChargeColor;
            if (level == null) return OfflineColor;
            if (level <= s.CriticalPercent) return BadColor;
            if (level <= s.LowPercent) return WarnColor;
            return GoodColor;
        }

        public static SolidColorBrush LevelBrush(int? level, bool charging, bool online, AppSettings s)
        {
            Color c = LevelColor(level, charging, online, s);
            return c == GoodColor ? Good : c == WarnColor ? Warn : c == BadColor ? Bad : c == ChargeColor ? Charge : Offline;
        }

        /// <summary>Segoe Fluent Icons / MDL2 glyph for a device kind.</summary>
        public static string Glyph(DeviceKind kind)
        {
            return kind switch
            {
                DeviceKind.Headset => "",
                DeviceKind.Mouse => "",
                DeviceKind.Keyboard => "",
                DeviceKind.Gamepad => "",
                _ => "",
            };
        }

        [DllImport("dwmapi.dll")]
        static extern int DwmSetWindowAttribute(IntPtr hwnd, int attribute, ref int value, int size);

        /// <summary>Dark title bar to match the dark window (DWMWA_USE_IMMERSIVE_DARK_MODE).</summary>
        public static void UseDarkTitleBar(Window window)
        {
            IntPtr hwnd = new WindowInteropHelper(window).EnsureHandle();
            int on = 1;
            DwmSetWindowAttribute(hwnd, 20, ref on, sizeof(int));
        }
    }
}
