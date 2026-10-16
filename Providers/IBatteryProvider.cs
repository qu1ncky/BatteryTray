using System;
using System.Collections.Generic;
using BatteryTray.Core;

namespace BatteryTray.Providers
{
    public interface IBatteryProvider
    {
        string Name { get; }

        /// <summary>Queries the hardware (blocking; called on a worker thread) and returns every device this
        /// provider currently knows, online or not.</summary>
        IReadOnlyList<DeviceReading> Poll();

        /// <summary>How long to wait before the next poll, given what the last one returned.</summary>
        TimeSpan Delay(IReadOnlyList<DeviceReading> last);
    }

    /// <summary>A provider whose devices also report changes by themselves: it asks to be polled right away.</summary>
    public interface IPushingProvider
    {
        event Action? Pushed;
    }

    public static class ProviderList
    {
        public static IBatteryProvider[] Create(StateStore state)
        {
            return new IBatteryProvider[]
            {
                new RazerBarracudaProvider(state),
                new RongyuanDongleProvider(state),
                new SteelSeriesProvider(),
                new RazerPeripheralProvider(),
                new HeadsetProvider(),
                new LogitechProvider(),
                new PlayStationProvider(),
                new XInputProvider(),
                new BluetoothProvider(),
                new StandardHidBatteryProvider(),
            };
        }
    }
}
