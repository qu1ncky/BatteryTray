using System;
using System.Runtime.InteropServices;
using System.Windows.Interop;

namespace BatteryTray.UI
{
    /// <summary>
    /// Lets a second launch of the exe (Start menu, double-click) open the window of the instance already in the
    /// tray. The running instance owns a hidden message-only window; the new one posts it a message and exits.
    /// The running instance is elevated and the launcher usually isn't, so the message is explicitly let through
    /// UIPI (ChangeWindowMessageFilterEx), and the launcher hands over its right to bring a window to the front.
    /// </summary>
    public sealed class InstanceActivator : IDisposable
    {
        const string WindowTitle = "BatteryTray.Activator";
        static readonly IntPtr HwndMessage = new(-3);
        static readonly uint OpenMessage = RegisterWindowMessage("BatteryTray.Open");

        readonly HwndSource source;

        public InstanceActivator(Action open)
        {
            source = new HwndSource(new HwndSourceParameters(WindowTitle) { ParentWindow = HwndMessage, WindowStyle = 0 });
            source.AddHook((IntPtr hwnd, int msg, IntPtr wParam, IntPtr lParam, ref bool handled) =>
            {
                if ((uint)msg == OpenMessage)
                {
                    open();
                    handled = true;
                }
                return IntPtr.Zero;
            });
            ChangeWindowMessageFilterEx(source.Handle, OpenMessage, MsgfltAllow, IntPtr.Zero);
        }

        /// <summary>Asks the running instance to open its window. False when no instance is running.</summary>
        public static bool SignalRunning()
        {
            IntPtr target = FindWindowEx(HwndMessage, IntPtr.Zero, null, WindowTitle);
            if (target == IntPtr.Zero) return false;
            AllowSetForegroundWindow(AsfwAny);
            return PostMessage(target, OpenMessage, IntPtr.Zero, IntPtr.Zero);
        }

        public void Dispose()
        {
            source.Dispose();
        }

        const uint MsgfltAllow = 1;
        const int AsfwAny = -1;

        [DllImport("user32.dll", CharSet = CharSet.Unicode)]
        static extern uint RegisterWindowMessage(string name);
        [DllImport("user32.dll", CharSet = CharSet.Unicode)]
        static extern IntPtr FindWindowEx(IntPtr parent, IntPtr after, string? className, string title);
        [DllImport("user32.dll")]
        static extern bool PostMessage(IntPtr hwnd, uint msg, IntPtr wParam, IntPtr lParam);
        [DllImport("user32.dll")]
        static extern bool ChangeWindowMessageFilterEx(IntPtr hwnd, uint msg, uint action, IntPtr info);
        [DllImport("user32.dll")]
        static extern bool AllowSetForegroundWindow(int processId);
    }
}
