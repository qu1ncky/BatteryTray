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
        readonly DispatcherTimer clock = new() { Interval = TimeSpan.FromSeconds(15) }; // keeps "N мин назад" fresh
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
            Closed += (_, _) => clock.Stop();
            LoadSettings();
            Refresh();
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
            UpdatedText.Text = newest is DateTime t ? $"Обновлено в {t:HH:mm:ss}" : "Опрашиваю устройства…";

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
            AutoStartToggle.IsChecked = AutoStart.IsRegistered();
            ElevationText.Text = Elevation.IsElevated
                ? "Программа работает с правами администратора, поэтому видит и устройства, доступ к которым " +
                  "закрыли сторонние драйверы. При входе в Windows права выдаются автоматически, без окна UAC."
                : AutoStart.IsInstalled
                    ? "Программа запущена без прав администратора. Если у устройства написано «нужны права " +
                      "администратора», запустите её из меню «Пуск» — она поднимется через задачу автозапуска с правами."
                    : "Это переносная копия: автозапуск будет с обычными правами. Если у устройства написано " +
                      "«нужны права администратора», установите программу через install.cmd из архива с релизом.";
            UpdateLabels();
            loading = false;
        }

        void UpdateLabels()
        {
            LowValue.Text = $"{(int)LowSlider.Value}%";
            CriticalValue.Text = $"{(int)CriticalSlider.Value}%";
            RepeatValue.Text = $"{(int)RepeatSlider.Value} мин";
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
            UpdateLabels();
            app.SettingsChanged();
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
