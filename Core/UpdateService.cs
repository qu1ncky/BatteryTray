using System;
using System.Threading.Tasks;
using System.Windows.Threading;

namespace BatteryTray.Core
{
    /// <summary>
    /// Checks for a new release shortly after start and then every few hours. With automatic updates on, a new
    /// version is installed and the app restarts into it - but only while no window of the app is open, so it never
    /// vanishes from under the user. When installing needs an admin prompt (Program Files without admin rights),
    /// the update waits for the user's click instead and is announced once.
    /// </summary>
    public sealed class UpdateService
    {
        public enum Stage { NotChecked, Checking, UpToDate, Available, Installing, Failed }

        static readonly TimeSpan FirstCheck = TimeSpan.FromSeconds(30);
        static readonly TimeSpan Period = TimeSpan.FromHours(6);
        static readonly TimeSpan Retry = TimeSpan.FromMinutes(30);

        readonly AppSettings settings;
        readonly Func<bool> windowOpen;
        readonly Action<ReleaseInfo> announce;
        readonly Action<ReleaseInfo> installed;
        readonly DispatcherTimer timer = new();
        string? announcedTag;

        public UpdateService(AppSettings settings, Func<bool> windowOpen, Action<ReleaseInfo> announce,
            Action<ReleaseInfo> installed)
        {
            this.settings = settings;
            this.windowOpen = windowOpen;
            this.announce = announce;
            this.installed = installed;
            timer.Tick += async (_, _) =>
            {
                timer.Stop();
                await CheckAsync();
            };
        }

        public Stage State { get; private set; } = Stage.NotChecked;
        public ReleaseInfo? Latest { get; private set; }
        public DateTime? LastCheck { get; private set; }
        public int Progress { get; private set; }
        public string? Error { get; private set; }

        /// <summary>A newer version than the running one is known.</summary>
        public bool NewerAvailable => Latest != null && Latest.Version > Updater.Current;

        public event Action? Changed;

        public void Start()
        {
            Schedule(FirstCheck);
        }

        public async Task CheckAsync()
        {
            if (State is Stage.Checking or Stage.Installing) return;
            Set(Stage.Checking);
            try
            {
                Latest = await Updater.GetLatestAsync();
                LastCheck = DateTime.Now;
                Error = null;
            }
            catch (Exception e)
            {
                Error = e.Message;
                Log.Write("update check failed: " + e.Message);
                Set(Stage.Failed);
                Schedule(Retry);
                return;
            }

            if (!NewerAvailable)
            {
                Set(Stage.UpToDate);
                Schedule(Period);
                return;
            }

            Log.Write($"update available: {Latest!.Tag}");
            Set(Stage.Available);
            if (settings.AutoUpdate && Updater.CanInstallInPlace())
            {
                if (!windowOpen())
                {
                    await InstallAsync();
                    return;
                }
                Schedule(Retry); // the user is looking at a window: try again later
                return;
            }
            if (announcedTag != Latest.Tag)
            {
                announcedTag = Latest.Tag;
                announce(Latest);
            }
            Schedule(Period);
        }

        public async Task InstallAsync()
        {
            if (!NewerAvailable || State == Stage.Installing) return;
            ReleaseInfo release = Latest!;
            Progress = 0;
            Set(Stage.Installing);
            try
            {
                if (Updater.CanInstallInPlace())
                {
                    var progress = new Progress<int>(p =>
                    {
                        Progress = p;
                        Changed?.Invoke();
                    });
                    await Task.Run(() => Updater.InstallAsync(release, progress));
                }
                // Program Files without admin rights: an elevated copy of this exe does the same (UAC prompt)
                else if (!await Task.Run(() => AutoStart.RunSelfElevated(CommandLine.UpdateSwitch)))
                {
                    throw new InvalidOperationException("обновление отменено или не удалось, подробности в логе");
                }
            }
            catch (Exception e)
            {
                Error = e.Message;
                Log.Write("update failed: " + e);
                Set(Stage.Failed);
                Schedule(Retry);
                return;
            }
            installed(release);
        }

        void Schedule(TimeSpan after)
        {
            timer.Stop();
            timer.Interval = after;
            timer.Start();
        }

        void Set(Stage stage)
        {
            State = stage;
            Changed?.Invoke();
        }
    }
}
