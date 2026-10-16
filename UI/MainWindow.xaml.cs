using System;
using System.Collections.Generic;
using System.Collections.ObjectModel;
using System.Diagnostics;
using System.Linq;
using System.Windows;
using System.Windows.Threading;
using BatteryTray.Core;

namespace BatteryTray.UI
{
    public partial class MainWindow : Window
    {
        readonly App app;
        readonly ObservableCollection<DeviceVM> online = new();
        readonly ObservableCollection<DeviceVM> offline = new();
        readonly Dictionary<string, DeviceVM> cards = new();
        readonly ObservableCollection<DeviceSettingsVM> deviceSettings = new();
        readonly DispatcherTimer clock = new() { Interval = TimeSpan.FromSeconds(15) }; // keeps "N min ago" fresh
        bool loading = true; // sliders fire ValueChanged while the XAML is still loading

        public MainWindow(App app)
        {
            this.app = app;
            InitializeComponent();
            OnlineList.ItemsSource = online;
            OfflineList.ItemsSource = offline;
            DeviceSettingsList.ItemsSource = deviceSettings;
            VersionText.Text = "BatteryTray " + App.Version;
            SourceInitialized += (_, _) => Theme.UseDarkTitleBar(this);
            clock.Tick += (_, _) => Refresh();
            clock.Start();
            Loc.Changed += OnLanguageChanged;
            Closed += (_, _) =>
            {
                clock.Stop();
                Loc.Changed -= OnLanguageChanged;
            };
            LoadSettings();
            Refresh();
            RefreshUpdate();
        }

        public void ShowAndActivate()
        {
            if (!IsVisible)
            {
                Rect area = SystemParameters.WorkArea;
                Left = area.Right - Width - 12;
                Top = area.Bottom - Height - 12;
                Show();
            }
            if (WindowState == WindowState.Minimized) WindowState = WindowState.Normal;
            Activate();
        }

        public void Refresh()
        {
            DateTime now = DateTime.Now;
            AppSettings s = app.Settings;
            IReadOnlyList<DeviceRow> rows = app.Rows();
            var shown = rows.Where(r => !s.For(r.Id).Hidden).ToList();

            Sync(online, shown.Where(r => r.Online), s, now);
            Sync(offline, shown.Where(r => !r.Online), s, now);
            NoOnline.Visibility = online.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
            OfflineHeader.Visibility = offline.Count > 0 ? Visibility.Visible : Visibility.Collapsed;

            DateTime? newest = rows.Select(r => r.Current?.Time).Max();
            UpdatedText.Text = newest is DateTime t ? Loc.T("updated.at", t) : Loc.T("polling");

            // Rebuilt only when the set of devices changes, so a name being typed keeps its focus
            if (!rows.Select(r => r.Id).SequenceEqual(deviceSettings.Select(d => d.Id)))
            {
                deviceSettings.Clear();
                foreach (var row in rows) deviceSettings.Add(new DeviceSettingsVM(row, s.For(row.Id), app.SettingsChanged));
            }
        }

        void Sync(ObservableCollection<DeviceVM> target, IEnumerable<DeviceRow> rows, AppSettings s, DateTime now)
        {
            var list = rows.Select(r =>
            {
                if (!cards.TryGetValue(r.Id, out var vm)) cards[r.Id] = vm = new DeviceVM(r.Id);
                vm.Update(r, s, now);
                return vm;
            }).ToList();
            if (list.SequenceEqual(target)) return;
            target.Clear();
            foreach (var vm in list) target.Add(vm);
        }

        void LoadSettings()
        {
            loading = true;
            AppSettings s = app.Settings;
            LowSlider.Value = s.LowPercent;
            CriticalSlider.Value = s.CriticalPercent;
            RepeatSlider.Value = s.RepeatMinutes;
            SoundToggle.IsChecked = s.Sound;
            ChargedToggle.IsChecked = s.NotifyCharged;
            AutoUpdateToggle.IsChecked = s.AutoUpdate;
            AutoStartToggle.IsChecked = AutoStart.IsRegistered();
            string language = s.Language ?? Loc.Auto;
            foreach (var option in new[] { LangAuto, LangRu, LangEn, LangZh })
                option.IsChecked = (string)option.Tag == language || (option == LangAuto && Array.IndexOf(Loc.Languages, language) < 0);
            ElevationText.Text = Loc.T(Elevation.IsElevated ? "elev.admin" : AutoStart.IsInstalled ? "elev.installed" : "elev.portable");
            UpdateLabels();
            loading = false;
        }

        void UpdateLabels()
        {
            LowValue.Text = $"{(int)LowSlider.Value}%";
            CriticalValue.Text = $"{(int)CriticalSlider.Value}%";
            RepeatValue.Text = Loc.T("minutes", (int)RepeatSlider.Value);
        }

        void Setting_Changed(object sender, RoutedEventArgs e)
        {
            if (loading) return;
            loading = true;
            // The critical level must stay below the reminder level
            if (sender == LowSlider && CriticalSlider.Value >= LowSlider.Value) CriticalSlider.Value = LowSlider.Value - 5;
            if (sender == CriticalSlider && CriticalSlider.Value >= LowSlider.Value) LowSlider.Value = CriticalSlider.Value + 5;
            loading = false;

            AppSettings s = app.Settings;
            s.LowPercent = (int)LowSlider.Value;
            s.CriticalPercent = (int)CriticalSlider.Value;
            s.RepeatMinutes = (int)RepeatSlider.Value;
            s.Sound = SoundToggle.IsChecked == true;
            s.NotifyCharged = ChargedToggle.IsChecked == true;
            s.AutoUpdate = AutoUpdateToggle.IsChecked == true;
            UpdateLabels();
            app.SettingsChanged();
        }

        public void RefreshUpdate()
        {
            UpdateService u = app.Updates;
            string latest = u.Latest?.Version.ToString(3) ?? "";
            UpdateStatusText.Text = u.State switch
            {
                UpdateService.Stage.Checking => Loc.T("upd.checking"),
                UpdateService.Stage.UpToDate => Loc.T("upd.uptodate", App.Version, u.LastCheck),
                UpdateService.Stage.Available => Loc.T("upd.available", latest, App.Version),
                UpdateService.Stage.Installing => u.Progress is > 0 and < 100
                    ? Loc.T("upd.downloading", latest, u.Progress)
                    : Loc.T("upd.installing", latest),
                UpdateService.Stage.Failed => Loc.T("upd.failed", u.Error),
                _ => Loc.T("upd.idle", App.Version),
            };

            bool offer = u.NewerAvailable && u.State is UpdateService.Stage.Available or UpdateService.Stage.Failed;
            InstallUpdateButton.Content = Loc.T("upd.install", latest);
            InstallUpdateButton.Visibility = offer ? Visibility.Visible : Visibility.Collapsed;
            CheckUpdatesButton.IsEnabled = u.State is not (UpdateService.Stage.Checking or UpdateService.Stage.Installing);

            string notes = u.NewerAvailable ? Plain(u.Latest!.Notes) : "";
            UpdateNotesText.Text = notes.Length > 600 ? notes[..600].TrimEnd() + "…" : notes;
            UpdateNotesText.Visibility = notes.Length > 0 ? Visibility.Visible : Visibility.Collapsed;

            bool banner = u.NewerAvailable && u.State is UpdateService.Stage.Available or UpdateService.Stage.Installing;
            UpdateBanner.Visibility = banner ? Visibility.Visible : Visibility.Collapsed;
            UpdateBannerText.Text = u.State == UpdateService.Stage.Installing
                ? UpdateStatusText.Text
                : Loc.T("upd.banner", latest);
            UpdateBannerButton.Visibility = u.State == UpdateService.Stage.Available ? Visibility.Visible : Visibility.Collapsed;
        }

        /// <summary>Release notes are Markdown; show them as plain text.</summary>
        static string Plain(string markdown)
        {
            return markdown.Replace("**", "").Replace("`", "").Trim();
        }

        async void InstallUpdate_Click(object sender, RoutedEventArgs e)
        {
            await app.Updates.InstallAsync();
        }

        async void CheckUpdates_Click(object sender, RoutedEventArgs e)
        {
            await app.Updates.CheckAsync();
        }

        void Language_Checked(object sender, RoutedEventArgs e)
        {
            if (loading) return;
            app.SetLanguage((string)((FrameworkElement)sender).Tag);
        }

        /// <summary>XAML texts follow by themselves; the ones built here are built again.</summary>
        void OnLanguageChanged()
        {
            LoadSettings();
            deviceSettings.Clear(); // their source line holds texts from the providers
            Refresh();
            RefreshUpdate();
        }

        void Tab_Checked(object sender, RoutedEventArgs e)
        {
            if (DevicesView == null) return; // fires during InitializeComponent
            bool devices = sender == DevicesTab;
            DevicesView.Visibility = devices ? Visibility.Visible : Visibility.Collapsed;
            SettingsView.Visibility = devices ? Visibility.Collapsed : Visibility.Visible;
        }

        void Refresh_Click(object sender, RoutedEventArgs e)
        {
            app.Watcher.RefreshNow();
        }

        void TestAlert_Click(object sender, RoutedEventArgs e)
        {
            app.ShowTestAlert();
        }

        void AutoStart_Click(object sender, RoutedEventArgs e)
        {
            bool want = AutoStartToggle.IsChecked == true;
            Cursor = System.Windows.Input.Cursors.Wait;
            AutoStart.Set(want);
            AutoStartToggle.IsChecked = AutoStart.IsRegistered();
            Cursor = null;
        }

        void OpenFolder_Click(object sender, RoutedEventArgs e)
        {
            Process.Start(new ProcessStartInfo("explorer.exe", $"\"{Log.Folder}\"") { UseShellExecute = true });
        }
    }
}
