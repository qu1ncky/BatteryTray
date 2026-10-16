using System;
using System.Collections.Generic;
using System.Linq;
using System.Runtime.InteropServices;
using System.Windows;
using System.Windows.Interop;
using System.Windows.Media;
using System.Windows.Media.Animation;
using System.Windows.Threading;
using BatteryTray.Core;

namespace BatteryTray.UI
{
    /// <summary>
    /// The low-battery reminder: a small always-on-top window in the corner of the screen. Unlike a toast it isn't
    /// swallowed by Do Not Disturb and doesn't time out - it stays until the user picks an answer. It never takes
    /// focus, so it doesn't interrupt typing or a game.
    /// </summary>
    public partial class AlertWindow : Window
    {
        readonly DispatcherTimer keepOnTop = new() { Interval = TimeSpan.FromSeconds(3) };
        IReadOnlyList<AlertItem> items = Array.Empty<AlertItem>();

        public event Action? SnoozeClicked;
        public event Action? MuteClicked;

        public AlertWindow()
        {
            InitializeComponent();
            SourceInitialized += (_, _) => MakeToolWindow();
            SizeChanged += (_, _) => PlaceInCorner();
            Loaded += (_, _) => BeginAnimation(OpacityProperty, new DoubleAnimation(0, 1, TimeSpan.FromMilliseconds(220)));
            // Full-screen apps and other topmost windows can push it back; reassert every few seconds
            keepOnTop.Tick += (_, _) => SetWindowPos(new WindowInteropHelper(this).Handle, HwndTopmost, 0, 0, 0, 0,
                SwpNoMove | SwpNoSize | SwpNoActivate);
            keepOnTop.Start();
            Closed += (_, _) => keepOnTop.Stop();
        }

        public IReadOnlyList<string> Ids => items.Select(i => i.Id).ToList();

        /// <summary>True when the window already shows this device at this severity (no new sound needed).</summary>
        public bool Shows(AlertItem a) => items.Any(i => i.Id == a.Id && i.Critical == a.Critical);

        public void SetItems(IReadOnlyList<AlertItem> due, int repeatMinutes)
        {
            items = due;
            bool critical = due.Any(d => d.Critical);
            SolidColorBrush accent = critical ? Theme.Bad : Theme.Warn;
            TitleText.Text = Loc.T(critical ? "alert.title.critical" : due.Count == 1 ? "alert.title" : "alert.title.many");
            TitleGlyph.Foreground = accent;
            AccentBar.Background = accent;
            Frame.BorderBrush = new SolidColorBrush(Color.FromArgb(0x80, accent.Color.R, accent.Color.G, accent.Color.B));
            Items.ItemsSource = due.Select(d => new
            {
                Glyph = Theme.Glyph(d.Kind),
                d.Name,
                Level = $"{d.Level}%",
                Brush = d.Critical ? Theme.Bad : Theme.Warn,
            }).ToList();
            SnoozeButton.Content = Loc.T("alert.snooze", repeatMinutes);
            HintText.Text = Loc.T(critical ? "alert.hint.critical" : "alert.hint");
        }

        void Snooze_Click(object sender, RoutedEventArgs e)
        {
            SnoozeClicked?.Invoke();
        }

        void Mute_Click(object sender, RoutedEventArgs e)
        {
            MuteClicked?.Invoke();
        }

        void PlaceInCorner()
        {
            Rect area = SystemParameters.WorkArea;
            Left = area.Right - ActualWidth;
            Top = area.Bottom - ActualHeight;
        }

        void MakeToolWindow()
        {
            IntPtr hwnd = new WindowInteropHelper(this).Handle;
            long style = GetWindowLongPtr(hwnd, GwlExStyle).ToInt64();
            SetWindowLongPtr(hwnd, GwlExStyle, new IntPtr(style | WsExToolWindow | WsExNoActivate));
        }

        const int GwlExStyle = -20;
        const long WsExToolWindow = 0x80, WsExNoActivate = 0x08000000;
        static readonly IntPtr HwndTopmost = new(-1);
        const uint SwpNoSize = 0x1, SwpNoMove = 0x2, SwpNoActivate = 0x10;

        [DllImport("user32.dll", EntryPoint = "GetWindowLongPtrW")]
        static extern IntPtr GetWindowLongPtr(IntPtr hwnd, int index);
        [DllImport("user32.dll", EntryPoint = "SetWindowLongPtrW")]
        static extern IntPtr SetWindowLongPtr(IntPtr hwnd, int index, IntPtr value);
        [DllImport("user32.dll")]
        static extern bool SetWindowPos(IntPtr hwnd, IntPtr after, int x, int y, int cx, int cy, uint flags);
    }
}
