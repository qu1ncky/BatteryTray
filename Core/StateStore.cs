using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Text.Encodings.Web;
using System.Text.Json;

namespace BatteryTray.Core
{
    /// <summary>
    /// The last reading with a known level for every device we have seen, kept across restarts
    /// (%LOCALAPPDATA%\BatteryTray\state.json), so a device that is off still shows where it was left.
    /// </summary>
    public sealed class StateStore
    {
        readonly object gate = new();
        readonly Dictionary<string, DeviceReading> last;
        bool dirty;

        static string FilePath => Path.Combine(Log.Folder, "state.json");

        static readonly JsonSerializerOptions Json = new()
        {
            WriteIndented = true,
            Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
        };

        StateStore(Dictionary<string, DeviceReading> last)
        {
            this.last = last;
        }

        public static StateStore Load()
        {
            try
            {
                if (File.Exists(FilePath))
                {
                    var data = JsonSerializer.Deserialize<Dictionary<string, DeviceReading>>(File.ReadAllText(FilePath), Json);
                    if (data != null) return new StateStore(data);
                }
            }
            catch (Exception e)
            {
                Log.Write("state.json unreadable, starting empty: " + e.Message);
            }
            return new StateStore(new Dictionary<string, DeviceReading>());
        }

        public IReadOnlyList<string> Ids
        {
            get { lock (gate) return last.Keys.ToList(); }
        }

        public DeviceReading? Get(string id)
        {
            lock (gate) return last.TryGetValue(id, out var r) ? r : null;
        }

        public void Update(DeviceReading r)
        {
            if (!r.Online || r.Level == null) return;
            lock (gate)
            {
                // Only a changed level is worth a write; the timestamp alone is saved on exit
                if (!last.TryGetValue(r.Id, out var old) || old.Level != r.Level || old.Charging != r.Charging
                    || old.Name != r.Name)
                    dirty = true;
                last[r.Id] = r;
            }
        }

        public void SaveIfDirty()
        {
            if (dirty) Save();
        }

        public void Save()
        {
            try
            {
                string text;
                lock (gate)
                {
                    text = JsonSerializer.Serialize(last, Json);
                    dirty = false;
                }
                Directory.CreateDirectory(Log.Folder);
                string tmp = FilePath + ".tmp";
                File.WriteAllText(tmp, text);
                File.Move(tmp, FilePath, overwrite: true);
            }
            catch (Exception e)
            {
                Log.Write("cannot save state: " + e.Message);
            }
        }
    }
}
