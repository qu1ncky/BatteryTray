using System;
using System.Collections.Generic;
using System.Windows;
using System.Windows.Threading;
using BatteryTray.Core;

namespace BatteryTray.UI
{
    /// <summary>Owns the HUD and its shortcut. Device refreshes update the contents without touching the visibility clock.</summary>
    public sealed class OverlayController : IDisposable
    {
        readonly AppSettings settings;
        readonly Func<IReadOnlyList<DeviceRow>> rows;
        readonly Func<Rect?> obstruction;
        readonly GlobalHotkey hotkey = new();
        readonly HotkeyBinding binding;
        readonly OverlaySession session = new();
        readonly DispatcherTimer expiry = new() { Interval = TimeSpan.FromMilliseconds(100) };
        readonly DispatcherTimer placement = new() { Interval = TimeSpan.FromSeconds(3) };
        OverlayWindow? window;
        OverlayMode mode;
        bool disposed, suspended;

        public event Action? Changed;
        public bool IsVisible => session.IsVisible;
        public bool ShortcutAvailable => binding.Active != null;

        public OverlayController(AppSettings settings, Func<IReadOnlyList<DeviceRow>> rows, Func<Rect?> obstruction)
        {
            this.settings = settings;
            this.rows = rows;
            this.obstruction = obstruction;
            binding = new HotkeyBinding(hotkey);
            hotkey.Pressed += OnHotkey;
            expiry.Tick += OnExpiry;
            placement.Tick += OnPlacement;
            Loc.Changed += Refresh;
            ApplySettings();
        }

        public void ApplySettings()
        {
            if (disposed) return;
            if (!settings.OverlayEnabled || mode != settings.OverlayMode) Hide();
            mode = settings.OverlayMode;
            if (!settings.OverlayEnabled) binding.Clear();
            else binding.TryChange(new HotkeyGesture(settings.OverlayHotkeyKey, settings.OverlayHotkeyModifiers));
            Refresh();
            Changed?.Invoke();
        }

        public bool TrySetShortcut(HotkeyGesture gesture)
        {
            if (disposed || !gesture.IsValid) return false;
            // Validate even while disabled, then release the trial registration again.
            if (!binding.TryChange(gesture)) return false;
            settings.OverlayHotkeyKey = gesture.Key;
            settings.OverlayHotkeyModifiers = gesture.Modifiers;
            if (!settings.OverlayEnabled) binding.Clear();
            Changed?.Invoke();
            return true;
        }

        public void SuspendShortcut(bool suspend)
        {
            suspended = suspend;
        }

        void OnHotkey(int id)
        {
            if (disposed || suspended || !binding.IsCurrent(id)) return;
            session.Press(mode);
            Present();
        }

        public void Preview()
        {
            if (disposed) return;
            session.Preview();
            Present();
        }

        void Present()
        {
            if (!session.IsVisible) { Hide(); return; }
            window ??= new OverlayWindow(obstruction);
            Refresh();
            if (!window.IsVisible) window.ShowOnActiveMonitor();
            if (session.IsTimed) expiry.Start();
            else expiry.Stop();
            placement.Start();
        }

        public void Refresh()
        {
            if (disposed || window == null || !session.IsVisible) return;
            window.SetItems(OverlayDevices.Select(rows(), settings), settings);
            if (window.IsVisible) window.Place();
        }

        void OnExpiry(object? sender, EventArgs e)
        {
            session.Expire();
            if (!session.IsVisible) Hide();
        }

        void OnPlacement(object? sender, EventArgs e) => window?.Place();

        void Hide()
        {
            session.Hide();
            expiry.Stop();
            placement.Stop();
            window?.Hide();
        }

        public void Dispose()
        {
            if (disposed) return;
            disposed = true;
            Hide();
            Loc.Changed -= Refresh;
            hotkey.Pressed -= OnHotkey;
            expiry.Tick -= OnExpiry;
            placement.Tick -= OnPlacement;
            binding.Dispose();
            hotkey.Dispose();
            window?.Close();
        }
    }
}
