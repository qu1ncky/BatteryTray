using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Runtime.CompilerServices;
using System.Windows;
using System.Windows.Media;
using BatteryTray.Core;

namespace BatteryTray.UI
{
    public abstract class Observable : INotifyPropertyChanged
    {
        public event PropertyChangedEventHandler? PropertyChanged;

        protected void Set<T>(ref T field, T value, [CallerMemberName] string? name = null)
        {
            if (EqualityComparer<T>.Default.Equals(field, value)) return;
            field = value;
            PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(name));
        }
    }

    /// <summary>One card on the devices tab.</summary>
    public sealed class DeviceVM : Observable
    {
        string name = "", glyph = "", subtitle = "", levelText = "";
        Brush levelBrush = Theme.Offline;
        GridLength fillWidth = new(0, GridUnitType.Star), restWidth = new(1, GridUnitType.Star);
        bool charging, indeterminate;
        double chargeGlyphSize = 16;

        public DeviceVM(string id)
        {
            Id = id;
        }

        public string Id { get; }
        public string Name { get => name; private set => Set(ref name, value); }
        public string Glyph { get => glyph; private set => Set(ref glyph, value); }
        public string Subtitle { get => subtitle; private set => Set(ref subtitle, value); }
        public string LevelText { get => levelText; private set => Set(ref levelText, value); }
        public Brush LevelBrush { get => levelBrush; private set => Set(ref levelBrush, value); }
        public bool Charging { get => charging; private set => Set(ref charging, value); }
        /// <summary>Charging, but the level can't be read right now: no number, a pulsing bar instead.</summary>
        public bool Indeterminate { get => indeterminate; private set => Set(ref indeterminate, value); }
        public double ChargeGlyphSize { get => chargeGlyphSize; private set => Set(ref chargeGlyphSize, value); }
        public GridLength FillWidth { get => fillWidth; private set => Set(ref fillWidth, value); }
        public GridLength RestWidth { get => restWidth; private set => Set(ref restWidth, value); }

        public void Update(DeviceRow row, AppSettings s, DateTime now)
        {
            Name = row.Name;
            Glyph = Theme.Glyph(row.Kind);
            var parts = new List<string>();
            int? level = null;
            bool estimate = false, unknown = false;
            if (row.Online)
            {
                DeviceReading cur = row.Current!;
                level = cur.Level;
                estimate = cur.Estimate;
                unknown = cur.Charging && level == null;
                if (cur.Charging) parts.Add(Loc.T("state.charging"));
                else if (cur.FullOnCharger) parts.Add(Loc.T("state.full"));
                parts.Add(cur.Via);
                if (!string.IsNullOrEmpty(cur.Detail)) parts.Add(cur.Detail);
                Charging = cur.Charging;
                LevelBrush = Theme.LevelBrush(level, cur.Charging, true, s);
            }
            else
            {
                // The reason, when the provider knows it ("asleep or off"), reads better than a bare "Not connected"
                string? why = row.Current?.Detail;
                parts.Add(string.IsNullOrEmpty(why) ? Loc.T("state.offline") : char.ToUpper(why[0]) + why[1..]);
                if (row.Last?.Level is int was) parts.Add(Loc.T("last.seen", was, Ago(row.Last.Time, now)));
                Charging = false;
                LevelBrush = Theme.Offline;
            }
            // An estimate is marked as one; while charging with no readable level the lightning alone says it all
            LevelText = level is int l ? (estimate ? "≈" : "") + $"{l}%" : unknown ? "" : "—";
            Indeterminate = unknown;
            ChargeGlyphSize = unknown ? 26 : 16;
            Subtitle = string.Join(" · ", parts);
            double fill = Math.Clamp((level ?? 0) / 100.0, 0, 1);
            FillWidth = new GridLength(fill, GridUnitType.Star);
            RestWidth = new GridLength(1 - fill, GridUnitType.Star);
        }

        static string Ago(DateTime t, DateTime now)
        {
            TimeSpan d = now - t;
            if (d.TotalMinutes < 1) return Loc.T("ago.now");
            if (d.TotalHours < 1) return Loc.T("ago.min", (int)d.TotalMinutes);
            if (d.TotalDays < 1) return Loc.T("ago.hours", (int)d.TotalHours);
            return Loc.T("ago.date", t);
        }
    }

    /// <summary>One device block on the settings tab; edits go straight into <see cref="AppSettings"/>.</summary>
    public sealed class DeviceSettingsVM : Observable
    {
        readonly DeviceSettings settings;
        readonly Action changed;

        public DeviceSettingsVM(DeviceRow row, DeviceSettings settings, Action changed)
        {
            this.settings = settings;
            this.changed = changed;
            Id = row.Id;
            DefaultName = row.Any.Name;
            Glyph = Theme.Glyph(row.Kind);
            Source = $"{row.Any.Via} · {row.Any.Name}";
        }

        public string Id { get; }
        public string DefaultName { get; }
        public string Glyph { get; }
        public string Source { get; }

        public string CustomName
        {
            get => settings.Name ?? "";
            set
            {
                settings.Name = string.IsNullOrWhiteSpace(value) ? null : value;
                changed();
            }
        }

        public bool Visible
        {
            get => !settings.Hidden;
            set
            {
                settings.Hidden = !value;
                changed();
            }
        }

        public bool Alerts
        {
            get => settings.Alerts;
            set
            {
                settings.Alerts = value;
                changed();
            }
        }
    }
}
