using System;
using System.Collections.Generic;
using System.IO;
using System.Text.Encodings.Web;
using System.Text.Json;

namespace BatteryTray.Core
{
    public sealed class DeviceSettings
    {
        /// <summary>Custom display name; null = the name the device reports.</summary>
        public string? Name { get; set; }
        public bool Hidden { get; set; }
        public bool Alerts { get; set; } = true;
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
