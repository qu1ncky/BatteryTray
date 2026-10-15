using System;
using System.Drawing;
using System.Drawing.Text;
using System.Runtime.InteropServices;
using System.Windows.Forms;

namespace BatteryTray.UI
{
    /// <summary>The notification-area icon: the lowest battery level as a colored number, with a small menu.</summary>
    public sealed class TrayIcon : IDisposable
    {
        readonly NotifyIcon icon = new();
        IntPtr iconHandle;

        public event Action? OpenRequested;
        public event Action? RefreshRequested;
        public event Action? ExitRequested;

        public TrayIcon()
        {
            var menu = new ContextMenuStrip
            {
                Renderer = new ToolStripProfessionalRenderer(new DarkMenuColors()),
                ShowImageMargin = false,
                Font = new Font("Segoe UI", 9.75f),
            };
            menu.Items.Add("Открыть", null, (_, _) => OpenRequested?.Invoke());
            menu.Items.Add("Обновить сейчас", null, (_, _) => RefreshRequested?.Invoke());
            menu.Items.Add(new ToolStripSeparator());
            menu.Items.Add("Выход", null, (_, _) => ExitRequested?.Invoke());
            foreach (ToolStripItem item in menu.Items)
            {
                item.ForeColor = Color.FromArgb(243, 243, 243);
                item.Padding = new Padding(4, 3, 4, 3);
            }

            icon.ContextMenuStrip = menu;
            icon.MouseClick += (_, e) =>
            {
                if (e.Button == MouseButtons.Left) OpenRequested?.Invoke();
            };
            Update("…", Color.Gray, "Заряд устройств");
            icon.Visible = true;
        }

        public void Update(string label, Color color, string tooltip)
        {
            icon.Text = tooltip.Length > 127 ? tooltip[..127] : tooltip;
            using Bitmap bmp = Render(SystemInformation.SmallIconSize, label, color);
            IntPtr old = iconHandle;
            iconHandle = bmp.GetHicon();
            icon.Icon = Icon.FromHandle(iconHandle);
            if (old != IntPtr.Zero) DestroyIcon(old);
        }

        public void ShowNotice(string title, string text)
        {
            icon.ShowBalloonTip(8000, title, text, ToolTipIcon.Info);
        }

        /// <summary>The label as large as fits the icon (from BarracudaBattery's IconRenderer, MIT).</summary>
        static Bitmap Render(Size size, string text, Color color)
        {
            var bmp = new Bitmap(size.Width, size.Height);
            using Graphics g = Graphics.FromImage(bmp);
            g.TextRenderingHint = TextRenderingHint.AntiAliasGridFit;
            g.Clear(Color.Transparent);
            StringFormat fmt = StringFormat.GenericTypographic;

            float em = size.Height;
            Font? font = null;
            SizeF measured;
            do
            {
                font?.Dispose();
                font = new Font("Segoe UI", em, FontStyle.Bold, GraphicsUnit.Pixel);
                measured = g.MeasureString(text, font, PointF.Empty, fmt);
                em -= 0.5f;
            } while ((measured.Width > size.Width || measured.Height > size.Height) && em > 4);

            using (font)
            using (var brush = new SolidBrush(color))
                g.DrawString(text, font, brush, (size.Width - measured.Width) / 2, (size.Height - measured.Height) / 2, fmt);
            return bmp;
        }

        public void Dispose()
        {
            icon.Visible = false;
            icon.Dispose();
            if (iconHandle != IntPtr.Zero) DestroyIcon(iconHandle);
        }

        [DllImport("user32.dll")]
        static extern bool DestroyIcon(IntPtr handle);

        sealed class DarkMenuColors : ProfessionalColorTable
        {
            static readonly Color Back = Color.FromArgb(43, 43, 43);
            static readonly Color Hover = Color.FromArgb(58, 58, 58);
            static readonly Color Line = Color.FromArgb(64, 64, 64);

            public override Color ToolStripDropDownBackground => Back;
            public override Color MenuBorder => Line;
            public override Color MenuItemBorder => Hover;
            public override Color MenuItemSelected => Hover;
            public override Color ImageMarginGradientBegin => Back;
            public override Color ImageMarginGradientMiddle => Back;
            public override Color ImageMarginGradientEnd => Back;
            public override Color SeparatorDark => Line;
            public override Color SeparatorLight => Back;
        }
    }
}
