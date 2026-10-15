using System;
using System.Collections.Generic;
using System.IO;
using BatteryTray.Providers;

namespace BatteryTray.Core
{
    /// <summary>
    /// Switches for the installer and for diagnostics:
    ///   --register-task / --unregister-task   create or remove the elevated logon task (needs admin)
    ///   --dump [file]                         poll every provider once and write the readings to a file
    /// </summary>
    public static class CommandLine
    {
        public static bool TryRun(string[] args, out int exitCode)
        {
            exitCode = 0;
            if (args.Length == 0) return false;
            switch (args[0])
            {
                case "--register-task":
                    exitCode = AutoStart.Register() ? 0 : 1;
                    return true;
                case "--unregister-task":
                    exitCode = AutoStart.Unregister() ? 0 : 1;
                    return true;
                case "--dump":
                    Dump(args.Length > 1 ? args[1] : Path.Combine(Log.Folder, "dump.txt"));
                    return true;
                default:
                    return false;
            }
        }

        static void Dump(string file)
        {
            var lines = new List<string> { $"{DateTime.Now:yyyy-MM-dd HH:mm:ss} elevated={Elevation.IsElevated}" };
            foreach (IBatteryProvider p in ProviderList.Create(StateStore.Load()))
            {
                try
                {
                    var readings = p.Poll();
                    if (readings.Count == 0) lines.Add($"[{p.Name}] nothing found");
                    foreach (var r in readings) lines.Add($"[{p.Name}] {r}");
                }
                catch (Exception e)
                {
                    lines.Add($"[{p.Name}] ERROR {e}");
                }
            }
            File.WriteAllLines(file, lines);
        }
    }
}
