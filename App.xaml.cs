using System;
using System.Collections.Generic;
using System.Linq;
using System.Media;
using System.Reflection;
using System.Threading;
using System.Windows;
using System.Windows.Threading;
using BatteryTray.Core;
using BatteryTray.Providers;
using BatteryTray.UI;
using Drawing = System.Drawing;

namespace BatteryTray
{
    /// <summary>Wires the monitor, the tray icon, the main window and the reminder window together.</summary>
    public partial class App : Application
    {
        public static string Version => Assembly.GetExecutingAssembly().GetName().Version?.ToString(3) ?? "?";

        Mutex? single;
        InstanceActivator? activator;
        TrayIcon? tray;
        MainWindow? window;
        AlertWindow? alertWindow;
        readonly AlertManager alerts = new();
        readonly DispatcherTimer alertTimer = new() { Interval = TimeSpan.FromSeconds(30) };
        readonly DispatcherTimer saveTimer = new() { Interval = TimeSpan.FromMilliseconds(500) };

        public AppSettings Settings { get; private set; } = new();
        public StateStore State { get; private set; } = null!;
        public BatteryMonitor Watcher { get; private set; } = null!;

        protected override void OnStartup(StartupEventArgs e)
        {
            base.OnStartup(e);
            if (CommandLine.TryRun(e.Args, out int exitCode))
            {
                Shutdown(exitCode);
                return;
            }

            // Launched by hand while the app already runs in the tray: open its window instead of starting again
            if (InstanceActivator.SignalRunning())
            {
                Shutdown();
                return;
            }
            // Launched by hand without admin rights: start the elevated copy through the logon task (no UAC
            // prompt), so the Razer headset can be read, and have it open its window
            if (!Elevation.IsElevated && AutoStart.IsRegistered() && AutoStart.RunTask())
            {
                for (int i = 0; i < 50; i++)
                {
                    Thread.Sleep(100);
                    if (!InstanceActivator.SignalRunning()) continue;
                    Shutdown();
                    return;
                }
                // The task brought nothing up (its exe was moved or deleted): run this copy instead
            }

            bool first;
            try
            {
                single = new Mutex(true, "BatteryTray.SingleInstance", out first);
            }
            catch (UnauthorizedAccessException)
            {
                first = false; // an elevated instance owns the mutex
            }
            if (!first)
            {
                Shutdown();
                return;
            }

            System.Windows.Forms.Application.EnableVisualStyles();
            DispatcherUnhandledException += (_, ex) =>
            {
                Log.Write("UI error: " + ex.Exception);
                ex.Handled = true;
            };

            Settings = AppSettings.Load();
            State = StateStore.Load();
            Watcher = new BatteryMonitor(ProviderList.Create(State), State);
            Watcher.Changed += OnDevicesChanged;

            tray = new TrayIcon();
            tray.OpenRequested += ShowMainWindow;
            tray.RefreshRequested += Watcher.RefreshNow;
            tray.ExitRequested += ExitApp;

            alertTimer.Tick += (_, _) => EvaluateAlerts(); // snoozes run out between polls
            alertTimer.Start();
            saveTimer.Tick += (_, _) =>
            {
                saveTimer.Stop();
                Settings.Save();
            };

            activator = new InstanceActivator(ShowMainWindow);

            Log.Write($"started v{Version}, elevated={Elevation.IsElevated}");
            Watcher.Start();
            UpdateTray();
            // The logon task starts it with --background; started any other way, show the window
            if (!e.Args.Contains(AutoStart.BackgroundSwitch)) ShowMainWindow();
        }

        /// <summary>Every device we know: what the providers report now, plus devices only remembered from before.</summary>
        public IReadOnlyList<DeviceRow> Rows()
        {
            var current = new Dictionary<string, DeviceReading>();
            foreach (var r in Watcher.Current) current[r.Id] = r;
            var rows = new List<DeviceRow>();
            foreach (string id in current.Keys.Union(State.Ids))
            {
                current.TryGetValue(id, out var cur);
                DeviceReading? last = State.Get(id);
                DeviceReading any = (cur ?? last)!;
                rows.Add(new DeviceRow(id, cur, last, Settings.DisplayName(id, any.Name)));
            }
            return rows.OrderBy(r => r.Kind).ThenBy(r => r.Name, StringComparer.CurrentCultureIgnoreCase).ToList();
        }

        public void SettingsChanged()
        {
            saveTimer.Stop();
            saveTimer.Start();
            UpdateTray();
            window?.Refresh();
            EvaluateAlerts();
        }

        public void ShowMainWindow()
        {
            if (window == null)
            {
                window = new MainWindow(this);
                window.Closed += (_, _) => window = null;
            }
            window.ShowAndActivate();
        }

        public void ShowTestAlert()
        {
            var sample = Rows().Where(r => r.Online && r.Current?.Level != null).Take(2)
                .Select((r, i) => new AlertItem(r.Id, r.Name, r.Kind, i == 0 ? Settings.LowPercent - 2 : Settings.CriticalPercent - 1, i > 0))
                .ToList();
            if (sample.Count == 0)
                sample.Add(new AlertItem("test", "Мышь", DeviceKind.Mouse, Settings.LowPercent - 2, false));
            var test = new AlertWindow();
            test.SnoozeClicked += test.Close;
            test.MuteClicked += test.Close;
            test.SetItems(sample, Settings.RepeatMinutes);
            test.Show();
            PlaySound(sample.Any(s => s.Critical));
        }

        void OnDevicesChanged()
        {
            UpdateTray();
            window?.Refresh();
            EvaluateAlerts();
            State.SaveIfDirty();
        }

        void UpdateTray()
        {
            if (tray == null) return;
            var live = Rows().Where(r => r.Online && r.Current!.Level != null && !Settings.For(r.Id).Hidden).ToList();
            string tip = live.Count == 0
                ? "Заряд устройств: нет данных"
                : string.Join("\n", live.Select(r => $"{Short(r.Name)}: {r.Current!.Level}%{(r.Current.Charging ? " ⚡" : "")}"));

            // The icon shows the device that will run out first; charging ones only when nothing else is left
            DeviceRow? pick = live.Where(r => !r.Current!.Charging).OrderBy(r => r.Current!.Level).FirstOrDefault()
                              ?? live.OrderBy(r => r.Current!.Level).FirstOrDefault();
            if (pick == null)
            {
                tray.Update("--", Drawing.Color.Gray, tip);
                return;
            }
            var c = Theme.LevelColor(pick.Current!.Level, pick.Current.Charging, true, Settings);
            tray.Update(pick.Current.Level!.Value.ToString(), Drawing.Color.FromArgb(c.R, c.G, c.B), tip);
        }

        static string Short(string name) => name.Length > 24 ? name[..23] + "…" : name;

        void EvaluateAlerts()
        {
            if (Watcher == null) return;
            IReadOnlyList<AlertItem> due = alerts.Due(Rows(), Settings, DateTime.Now);
            if (due.Count == 0)
            {
                CloseAlert();
                return;
            }

            bool fresh = alertWindow == null || due.Any(d => !alertWindow.Shows(d));
            if (alertWindow == null)
            {
                var w = new AlertWindow();
                w.SnoozeClicked += () =>
                {
                    alerts.Snooze(w.Ids, TimeSpan.FromMinutes(Settings.RepeatMinutes));
                    Log.Write($"alert snoozed for {Settings.RepeatMinutes} min");
                    CloseAlert();
                };
                w.MuteClicked += () =>
                {
                    alerts.MuteUntilCharged(w.Ids);
                    Log.Write("alert muted until charged");
                    CloseAlert();
                };
                alertWindow = w;
            }
            alertWindow.SetItems(due, Settings.RepeatMinutes);
            if (!alertWindow.IsVisible) alertWindow.Show();
            if (fresh)
            {
                Log.Write("alert: " + string.Join(", ", due.Select(d => $"{d.Name} {d.Level}%")));
                PlaySound(due.Any(d => d.Critical));
            }
        }

        void PlaySound(bool critical)
        {
            if (!Settings.Sound) return;
            (critical ? SystemSounds.Hand : SystemSounds.Exclamation).Play();
        }

        void CloseAlert()
        {
            alertWindow?.Close();
            alertWindow = null;
        }

        void ExitApp()
        {
            Log.Write("exit");
            Watcher.Dispose();
            State.Save();
            Settings.Save();
            CloseAlert();
            window?.Close();
            tray?.Dispose();
            activator?.Dispose();
            Shutdown();
        }
    }
}
