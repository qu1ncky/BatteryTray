using System;
using System.Collections.Generic;
using System.Linq;
using System.Windows;

namespace BatteryTray.Core
{
    public enum OverlayMode { Timed, Toggle }
    public enum OverlayCorner { TopLeft, TopRight, BottomLeft, BottomRight }

    /// <summary>Win32 virtual key and modifiers, without the registration-only MOD_NOREPEAT flag.</summary>
    public readonly record struct HotkeyGesture(int Key, int Modifiers)
    {
        public const int Alt = 0x0001, Control = 0x0002, Shift = 0x0004;
        public static readonly HotkeyGesture Default = new(0x56, Alt | Control); // Ctrl+Alt+V

        public bool IsValid => (Modifiers & ~(Alt | Control | Shift)) == 0
            && Key is >= 0x08 and <= 0xFE
            && Key is not (0x10 or 0x11 or 0x12 or 0x14 or 0x5B or 0x5C or 0x5D or 0x7B or 0x90 or 0x91)
            && Key is not (>= 0xA0 and <= 0xA5)
            && ((Modifiers & (Alt | Control)) != 0 || Key is >= 0x70 and <= 0x87)
            // Leave the standard clipboard shortcuts and Alt+F4 with the focused app.
            && !(Modifiers == Control && Key is (0x43 or 0x56 or 0x58))
            && !(Modifiers == Alt && Key == 0x73);
    }

    public interface IHotkeyRegistration
    {
        bool Register(int id, HotkeyGesture gesture);
        void Unregister(int id);
    }

    /// <summary>Registers the replacement first, so an occupied shortcut never costs the working binding.</summary>
    public sealed class HotkeyBinding : IDisposable
    {
        readonly IHotkeyRegistration registration;
        int activeId;

        public HotkeyBinding(IHotkeyRegistration registration) => this.registration = registration;
        public HotkeyGesture? Active { get; private set; }
        public bool IsCurrent(int id) => Active != null && id == activeId;

        public bool TryChange(HotkeyGesture gesture)
        {
            if (!gesture.IsValid) return false;
            if (Active == gesture) return true;
            int next = activeId == 1 ? 2 : 1;
            if (!registration.Register(next, gesture)) return false;
            if (Active != null) registration.Unregister(activeId);
            activeId = next;
            Active = gesture;
            return true;
        }

        public void Clear()
        {
            if (Active != null) registration.Unregister(activeId);
            Active = null;
        }

        public void Dispose() => Clear();
    }

    /// <summary>Visibility uses a monotonic clock: hardware updates and wall-clock changes cannot extend a peek.</summary>
    public sealed class OverlaySession
    {
        public static readonly TimeSpan Duration = TimeSpan.FromSeconds(5);
        readonly TimeProvider clock;
        long started;
        bool timed;

        public OverlaySession(TimeProvider? clock = null) => this.clock = clock ?? TimeProvider.System;
        public bool IsVisible { get; private set; }
        public bool IsTimed => IsVisible && timed;

        public void Press(OverlayMode mode)
        {
            if (mode == OverlayMode.Toggle && IsVisible) Hide();
            else Show(mode == OverlayMode.Timed);
        }

        public void Preview() => Show(true);

        void Show(bool expire)
        {
            IsVisible = true;
            timed = expire;
            started = clock.GetTimestamp();
        }

        public void Hide() => IsVisible = false;

        public void Expire()
        {
            if (IsVisible && timed && clock.GetElapsedTime(started) >= Duration) Hide();
        }
    }

    public sealed record OverlayItem(DeviceRow Device, bool ShowName);

    public static class OverlayDevices
    {
        public static IReadOnlyList<OverlayItem> Select(IEnumerable<DeviceRow> rows, AppSettings settings)
        {
            var shown = rows.Where(r => r.Online && !settings.For(r.Id).Hidden)
                .OrderBy(r => r.Kind).ThenBy(r => r.Name, StringComparer.CurrentCultureIgnoreCase)
                .ThenBy(r => r.Id, StringComparer.Ordinal).ToList();
            var duplicates = shown.GroupBy(r => r.Kind).Where(g => g.Count() > 1).Select(g => g.Key).ToHashSet();
            return shown.Select(r => new OverlayItem(r, settings.OverlayShowNames || duplicates.Contains(r.Kind))).ToList();
        }
    }

    public static class OverlayPlacement
    {
        /// <summary>All arguments use the same units; the native window supplies physical pixels and a scaled margin.</summary>
        public static Point Place(Rect workArea, Size size, OverlayCorner corner, double margin, Rect? obstruction = null)
        {
            double inset = Math.Min(margin, Math.Min(workArea.Width, workArea.Height) / 2);
            var area = new Rect(workArea.Left + inset, workArea.Top + inset,
                Math.Max(0, workArea.Width - inset * 2), Math.Max(0, workArea.Height - inset * 2));
            double width = Math.Min(size.Width, area.Width), height = Math.Min(size.Height, area.Height);
            bool right = corner is OverlayCorner.TopRight or OverlayCorner.BottomRight;
            bool bottom = corner is OverlayCorner.BottomLeft or OverlayCorner.BottomRight;
            var point = new Point(right ? area.Right - width : area.Left, bottom ? area.Bottom - height : area.Top);
            if (obstruction is not Rect blocked || !new Rect(point, new Size(width, height)).IntersectsWith(blocked))
                return point;

            var alternatives = new[]
            {
                new Point(point.X, blocked.Top - margin - height),
                new Point(point.X, blocked.Bottom + margin),
                new Point(blocked.Left - margin - width, point.Y),
                new Point(blocked.Right + margin, point.Y),
            };
            return alternatives.Where(p => area.Contains(new Rect(p, new Size(width, height)))
                    && !new Rect(p, new Size(width, height)).IntersectsWith(blocked))
                .OrderBy(p => (p - point).LengthSquared).Cast<Point?>().FirstOrDefault() ?? point;
        }
    }
}
