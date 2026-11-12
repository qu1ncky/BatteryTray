using System;
using System.Collections.Generic;
using System.Linq;
using System.Runtime.InteropServices;
using System.Windows;
using System.Windows.Interop;
using System.Windows.Threading;
using BatteryTray.Core;
using Forms = System.Windows.Forms;

namespace BatteryTray.UI
{
    /// <summary>Click-through, non-activating desktop HUD. Placement stays in native pixels across mixed-DPI displays.</summary>
    public partial class OverlayWindow : Window
    {
        readonly Func<Rect?> obstruction;
        string? monitor;
        OverlayCorner corner;
        bool placing, queued;

        public OverlayWindow(Func<Rect?> obstruction)
        {
            this.obstruction = obstruction;
            InitializeComponent();
            SourceInitialized += (_, _) =>
            {
                IntPtr hwnd = new WindowInteropHelper(this).Handle;
                long style = GetWindowLongPtr(hwnd, GwlExStyle).ToInt64();
                // WPF's transparent window is layered; WS_EX_TRANSPARENT passes input to windows underneath it.
                SetWindowLongPtr(hwnd, GwlExStyle, new IntPtr(style | WsExToolWindow | WsExTransparent | WsExNoActivate));
                HwndSource.FromHwnd(hwnd)?.AddHook(OnMessage);
            };
            SizeChanged += (_, _) => QueuePlacement();
        }

        public void SetItems(IReadOnlyList<OverlayItem> items, AppSettings settings)
        {
            corner = settings.OverlayCorner;
            Items.ItemsSource = items.Select(item =>
            {
                DeviceRow row = item.Device;
                DeviceReading current = row.Current!;
                return new
                {
                    row.Name,
                    Glyph = Theme.Glyph(row.Kind),
                    Level = current.Level is int level ? (current.Estimate ? "≈" : "") + level + "%"
                        : current.Charging || current.FullOnCharger ? "" : "—",
                    Brush = Theme.LevelBrush(current.Level, current.Charging || current.FullOnCharger, true, row.Id, settings),
                    NameVisibility = item.ShowName ? Visibility.Visible : Visibility.Collapsed,
                    PowerVisibility = current.Charging || current.FullOnCharger ? Visibility.Visible : Visibility.Collapsed,
                };
            }).ToList();
            EmptyText.Visibility = items.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
            QueuePlacement();
        }

        public void ShowOnActiveMonitor()
        {
            monitor = Forms.Screen.FromHandle(GetForegroundWindow()).DeviceName;
            IntPtr hwnd = new WindowInteropHelper(this).EnsureHandle();
            var screen = TargetScreen();
            // Establish the target DPI while still hidden, before measuring the contents.
            SetWindowPos(hwnd, IntPtr.Zero, screen.WorkingArea.Left, screen.WorkingArea.Top, 0, 0,
                SwpNoSize | SwpNoActivate | SwpNoZOrder);
            ConfigureSize(screen, hwnd);
            UpdateLayout();
            Place();
            Show();
            UpdateLayout();
            Place();
        }

        Forms.Screen TargetScreen() => Forms.Screen.AllScreens.FirstOrDefault(s => s.DeviceName == monitor)
            ?? Forms.Screen.PrimaryScreen ?? Forms.Screen.AllScreens[0];

        void ConfigureSize(Forms.Screen screen, IntPtr hwnd)
        {
            double scale = GetDpiForWindow(hwnd) / 96.0;
            if (scale <= 0) scale = 1;
            double width = Math.Max(1, screen.WorkingArea.Width / scale - 24);
            double height = Math.Max(1, screen.WorkingArea.Height / scale - 24);
            Fit.MaxWidth = width;
            Fit.MaxHeight = height;
            Frame.MaxWidth = Math.Min(width, Math.Max(240, screen.WorkingArea.Width / scale * 0.5));
        }

        public void Place()
        {
            if (placing) return;
            IntPtr hwnd = new WindowInteropHelper(this).Handle;
            if (hwnd == IntPtr.Zero) return;
            placing = true;
            try
            {
                Forms.Screen screen = TargetScreen();
                monitor = screen.DeviceName;
                // A removed display can leave the HWND at a different DPI. Move it first, then let WPF remeasure.
                if (Forms.Screen.FromHandle(hwnd).DeviceName != monitor)
                    SetWindowPos(hwnd, IntPtr.Zero, screen.WorkingArea.Left, screen.WorkingArea.Top, 0, 0,
                        SwpNoSize | SwpNoActivate | SwpNoZOrder);
                ConfigureSize(screen, hwnd);
                UpdateLayout();
                if (!GetWindowRect(hwnd, out NativeRect bounds)) return;
                var area = screen.WorkingArea;
                Point point = OverlayPlacement.Place(new Rect(area.X, area.Y, area.Width, area.Height),
                    new Size(bounds.Right - bounds.Left, bounds.Bottom - bounds.Top), corner,
                    12 * GetDpiForWindow(hwnd) / 96.0, obstruction());
                SetWindowPos(hwnd, HwndTopmost, (int)Math.Round(point.X), (int)Math.Round(point.Y), 0, 0,
                    SwpNoSize | SwpNoActivate);
            }
            finally { placing = false; }
        }

        void QueuePlacement()
        {
            if (queued || !IsVisible) return;
            queued = true;
            Dispatcher.BeginInvoke(DispatcherPriority.Loaded, new Action(() =>
            {
                queued = false;
                if (IsVisible) Place();
            }));
        }

        IntPtr OnMessage(IntPtr hwnd, int message, IntPtr wParam, IntPtr lParam, ref bool handled)
        {
            // Allow WPF to process the DPI change before measuring again.
            if (message is WmDpiChanged or WmDisplayChange or WmSettingChange) QueuePlacement();
            if (message == WmMouseActivate) { handled = true; return new IntPtr(MaNoActivate); }
            return IntPtr.Zero;
        }

        public static Rect? BoundsOf(Window? window)
        {
            if (window?.IsVisible != true) return null;
            IntPtr hwnd = new WindowInteropHelper(window).Handle;
            return GetWindowRect(hwnd, out NativeRect r) ? new Rect(r.Left, r.Top, r.Right - r.Left, r.Bottom - r.Top) : null;
        }

        const uint SwpNoSize = 0x1, SwpNoZOrder = 0x4, SwpNoActivate = 0x10;
        const int GwlExStyle = -20;
        const long WsExToolWindow = 0x80, WsExTransparent = 0x20, WsExNoActivate = 0x08000000;
        const int WmDpiChanged = 0x02E0, WmDisplayChange = 0x007E, WmSettingChange = 0x001A;
        const int WmMouseActivate = 0x0021, MaNoActivate = 3;
        static readonly IntPtr HwndTopmost = new(-1);
        [StructLayout(LayoutKind.Sequential)]
        struct NativeRect { public int Left, Top, Right, Bottom; }
        [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
        [DllImport("user32.dll")] static extern uint GetDpiForWindow(IntPtr hwnd);
        [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr hwnd, out NativeRect rect);
        [DllImport("user32.dll", EntryPoint = "GetWindowLongPtrW")] static extern IntPtr GetWindowLongPtr(IntPtr hwnd, int index);
        [DllImport("user32.dll", EntryPoint = "SetWindowLongPtrW")] static extern IntPtr SetWindowLongPtr(IntPtr hwnd, int index, IntPtr value);
        [DllImport("user32.dll")] static extern bool SetWindowPos(IntPtr hwnd, IntPtr after, int x, int y, int width, int height, uint flags);
    }
}
