using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using BatteryTray.Providers;

namespace BatteryTray.Core
{
    /// <summary>
    /// Polls every provider on its own schedule (HID queries block for seconds, so they never wait for each other)
    /// and raises <see cref="Changed"/> on the UI thread after each poll.
    /// </summary>
    public sealed class BatteryMonitor : IDisposable
    {
        readonly IBatteryProvider[] providers;
        readonly StateStore state;
        readonly SynchronizationContext ui;
        readonly object gate = new();
        readonly Dictionary<IBatteryProvider, IReadOnlyList<DeviceReading>> latest = new();
        readonly CancellationTokenSource[] wake;
        readonly Dictionary<string, string> loggedState = new();
        readonly CancellationTokenSource stop = new();

        public event Action? Changed;

        public BatteryMonitor(IEnumerable<IBatteryProvider> providers, StateStore state)
        {
            this.providers = providers.ToArray();
            this.state = state;
            ui = SynchronizationContext.Current ?? new SynchronizationContext();
            wake = this.providers.Select(_ => new CancellationTokenSource()).ToArray();
        }

        public void Start()
        {
            for (int i = 0; i < providers.Length; i++)
            {
                int index = i;
                Task.Run(() => Loop(index));
            }
        }

        /// <summary>Cuts every provider's wait short, so all of them poll right away.</summary>
        public void RefreshNow()
        {
            lock (gate)
                foreach (var w in wake) w.Cancel();
        }

        public IReadOnlyList<DeviceReading> Current
        {
            get { lock (gate) return latest.Values.SelectMany(r => r).ToList(); }
        }

        async Task Loop(int index)
        {
            IBatteryProvider provider = providers[index];
            while (!stop.IsCancellationRequested)
            {
                IReadOnlyList<DeviceReading> result;
                try
                {
                    result = provider.Poll();
                }
                catch (Exception e)
                {
                    Log.Write($"{provider.Name}: poll failed: {e}");
                    result = Array.Empty<DeviceReading>();
                }
                Publish(provider, result);

                CancellationTokenSource w;
                lock (gate)
                {
                    if (wake[index].IsCancellationRequested) wake[index] = new CancellationTokenSource();
                    w = wake[index];
                }
                using var linked = CancellationTokenSource.CreateLinkedTokenSource(stop.Token, w.Token);
                try
                {
                    await Task.Delay(provider.Delay(result), linked.Token);
                }
                catch (OperationCanceledException)
                {
                }
            }
        }

        void Publish(IBatteryProvider provider, IReadOnlyList<DeviceReading> result)
        {
            lock (gate)
            {
                latest[provider] = result;
                foreach (var r in result)
                {
                    state.Update(r);
                    // Log state changes only (not every poll), so the log shows the battery history at a glance
                    string summary = r.Online
                        ? $"на связи, {r.Level?.ToString() ?? "?"}%{(r.Charging ? ", заряжается" : "")}"
                        : "нет связи" + (r.Detail != null ? $" ({r.Detail})" : "");
                    if (!loggedState.TryGetValue(r.Id, out var old) || old != summary)
                    {
                        loggedState[r.Id] = summary;
                        Log.Write($"{r.Name} [{r.Via}]: {summary}");
                    }
                }
            }
            ui.Post(_ => Changed?.Invoke(), null);
        }

        public void Dispose()
        {
            stop.Cancel();
        }
    }
}
