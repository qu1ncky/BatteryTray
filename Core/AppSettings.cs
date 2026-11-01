using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Text.Encodings.Web;
using System.Text.Json;
using System.Text.Json.Serialization;

namespace BatteryTray.Core
{
    public sealed class DeviceSettings
    {
        /// <summary>Custom display name; null = the name the device reports.</summary>
        public string? Name { get; set; }
        public bool Hidden { get; set; }
        public bool Alerts { get; set; } = true;
        /// <summary>Reminder level for this device; null = the global <see cref="AppSettings.LowPercent"/>.</summary>
        [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
        public int? LowPercent { get; set; }
        /// <summary>Critical level for this device; null = the global <see cref="AppSettings.CriticalPercent"/>.</summary>
        [JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
        public int? CriticalPercent { get; set; }
    }

    /// <summary>User settings, stored as JSON in %LOCALAPPDATA%\BatteryTray\settings.json.</summary>
    public sealed class AppSettings
    {
        public int LowPercent { get; set; } = 20;
        public int CriticalPercent { get; set; } = 10;
        public int RepeatMinutes { get; set; } = 10;
        public bool Sound { get; set; } = true;
        /// <summary>"ru", "en", "zh", or "auto" to follow Windows.</summary>
        public string Language { get; set; } = Loc.Auto;
        /// <summary>A Windows notification when a device finishes charging.</summary>
        public bool NotifyCharged { get; set; } = true;
        public bool AutoUpdate { get; set; } = true;
        /// <summary>Version that ran last time; a different one means the app was just updated.</summary>
        public string? LastRunVersion { get; set; }
        public Dictionary<string, DeviceSettings> Devices { get; set; } = new();

        static string FilePath => Path.Combine(Log.Folder, "settings.json");

        static readonly JsonSerializerOptions Json = new()
        {
            WriteIndented = true,
            Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping, // keep Cyrillic names readable in the file
        };

        public DeviceSettings For(string id)
        {
            if (!Devices.TryGetValue(id, out var s)) Devices[id] = s = new DeviceSettings();
            return s;
        }

        /// <summary>The reminder level a device actually alerts at: its own when set, the global one otherwise.</summary>
        public int LowFor(string id) => Devices.GetValueOrDefault(id)?.LowPercent ?? LowPercent;

        /// <summary>The critical level a device actually turns red at: its own when set, the global one otherwise.
        /// Never closer than 5 below the level it alerts at, so a low custom level can't fire critical first.</summary>
        public int CriticalFor(string id) =>
            Math.Min(Devices.GetValueOrDefault(id)?.CriticalPercent ?? CriticalPercent, LowFor(id) - 5);

        public string DisplayName(string id, string fallback)
        {
            string? custom = Devices.TryGetValue(id, out var s) ? s.Name?.Trim() : null;
            return string.IsNullOrEmpty(custom) ? fallback : custom;
        }

        public static AppSettings Load()
        {
            AppSettings s;
            try
            {
                s = File.Exists(FilePath)
                    ? JsonSerializer.Deserialize<AppSettings>(File.ReadAllText(FilePath), Json) ?? new AppSettings()
                    : new AppSettings();
            }
            catch (Exception e)
            {
                Log.Write("settings.json unreadable, using defaults: " + e.Message);
                s = new AppSettings();
            }
            s.LowPercent = Math.Clamp(s.LowPercent, 10, 50);
            s.CriticalPercent = Math.Clamp(s.CriticalPercent, 5, s.LowPercent - 5);
            s.RepeatMinutes = Math.Clamp(s.RepeatMinutes, 1, 120);
            // A hand-edited file can hold nulls: dropped here, so they can't stop the app from starting
            s.Devices ??= new();
            foreach (string id in s.Devices.Where(d => d.Value == null).Select(d => d.Key).ToList())
                s.Devices.Remove(id);
            // Per-device levels ride the same rails. A device has both of its own levels or none: one set alone (by
            // hand) gets the other from the global ones, so its sliders show the levels it actually uses
            foreach (DeviceSettings d in s.Devices.Values)
            {
                if (d.LowPercent == null && d.CriticalPercent == null) continue;
                int low = Math.Clamp(d.LowPercent ?? s.LowPercent, 10, 50);
                d.LowPercent = low;
                d.CriticalPercent = Math.Clamp(d.CriticalPercent ?? s.CriticalPercent, 5, low - 5);
            }
            return s;
        }

        public void Save()
        {
            try
            {
                Directory.CreateDirectory(Log.Folder);
                string tmp = FilePath + ".tmp";
                File.WriteAllText(tmp, JsonSerializer.Serialize(this, Json));
                File.Move(tmp, FilePath, overwrite: true);
            }
            catch (Exception e)
            {
                Log.Write("cannot save settings: " + e.Message);
            }
        }
    }
}
