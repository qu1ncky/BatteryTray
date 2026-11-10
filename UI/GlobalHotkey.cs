using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Windows.Input;
using System.Windows.Interop;
using BatteryTray.Core;

namespace BatteryTray.UI
{
    /// <summary>A message-only window receives WM_HOTKEY even when all of the app's visible windows are closed.</summary>
    public sealed class GlobalHotkey : IHotkeyRegistration, IDisposable
    {
        const int WmHotkey = 0x0312;
        const uint ModNoRepeat = 0x4000;
        static readonly IntPtr HwndMessage = new(-3);
        readonly HwndSource source = new(new HwndSourceParameters("BatteryTray.OverlayHotkey")
        {
            ParentWindow = HwndMessage, WindowStyle = 0,
        });

        public event Action<int>? Pressed;

        public GlobalHotkey() => source.AddHook(OnMessage);

        IntPtr OnMessage(IntPtr hwnd, int message, IntPtr wParam, IntPtr lParam, ref bool handled)
        {
            if (message == WmHotkey)
            {
                handled = true;
                Pressed?.Invoke(wParam.ToInt32());
            }
            return IntPtr.Zero;
        }

        public bool Register(int id, HotkeyGesture gesture) =>
            RegisterHotKey(source.Handle, id, (uint)gesture.Modifiers | ModNoRepeat, (uint)gesture.Key);

        public void Unregister(int id) => UnregisterHotKey(source.Handle, id);

        public void Dispose()
        {
            source.RemoveHook(OnMessage);
            source.Dispose();
        }

        public static string Label(HotkeyGesture gesture)
        {
            var parts = new List<string>();
            if ((gesture.Modifiers & HotkeyGesture.Control) != 0) parts.Add("Ctrl");
            if ((gesture.Modifiers & HotkeyGesture.Alt) != 0) parts.Add("Alt");
            if ((gesture.Modifiers & HotkeyGesture.Shift) != 0) parts.Add("Shift");
            string? key = new KeyConverter().ConvertToString(KeyInterop.KeyFromVirtualKey(gesture.Key));
            parts.Add(string.IsNullOrEmpty(key) ? gesture.Key.ToString() : key);
            return string.Join("+", parts);
        }

        [DllImport("user32.dll", SetLastError = true)]
        static extern bool RegisterHotKey(IntPtr hwnd, int id, uint modifiers, uint key);
        [DllImport("user32.dll", SetLastError = true)]
        static extern bool UnregisterHotKey(IntPtr hwnd, int id);
    }
}
