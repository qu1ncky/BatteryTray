using System;
using System.IO;

namespace BatteryTray.Core
{
    /// <summary>Small rolling log in %LOCALAPPDATA%\BatteryTray (log.txt, the previous one kept as log.old.txt).</summary>
    public static class Log
    {
        const long MaxBytes = 512 * 1024;
        static readonly object Gate = new();

        public static readonly string Folder = Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "BatteryTray");

        public static void Write(string text)
        {
            lock (Gate)
            {
                try
                {
                    Directory.CreateDirectory(Folder);
                    string path = Path.Combine(Folder, "log.txt");
                    var file = new FileInfo(path);
                    if (file.Exists && file.Length > MaxBytes)
                    {
                        string old = Path.Combine(Folder, "log.old.txt");
                        File.Delete(old);
                        File.Move(path, old);
                    }
                    File.AppendAllText(path, $"{DateTime.Now:yyyy-MM-dd HH:mm:ss.fff}  {text}{Environment.NewLine}");
                }
                catch (Exception)
                {
                    // Logging must never break the app
                }
            }
        }
    }
}
