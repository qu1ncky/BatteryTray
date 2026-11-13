using BatteryTray.Providers;
using Xunit;
using static BatteryTray.Providers.SteelSeriesProvider;

namespace BatteryTray.Tests
{
    /// <summary>
    /// The Aerox 3 Wireless Gen 2 replies, as a Gen 2 owner logged them over a full discharge and charge in rivalcfg
    /// issue 276. Windows puts the report ID (none, so 00) in front of what rivalcfg prints.
    /// </summary>
    public sealed class SteelSeriesAeroxGen2Tests
    {
        const byte Wireless = 0xD2, Cable = 0x92;

        static Battery? Parse(byte command, params byte[] data)
        {
            var report = new byte[65];
            data.CopyTo(report, 1);
            return SteelSeriesProvider.Parse(report, Protocol.AeroxGen2, command);
        }

        [Theory]
        [InlineData(0x62, 98)]
        [InlineData(0x36, 54)]
        [InlineData(0x03, 3)]
        [InlineData(0x00, 0)]
        public void OnBatteryOverTheReceiver(byte level, int percent)
        {
            Assert.Equal(new Battery(percent, false, false), Parse(Wireless, 0xD2, 0x00, level, 0x00));
        }

        [Fact]
        public void ChargingOnTheCable()
        {
            Assert.Equal(new Battery(35, true, false), Parse(Cable, 0x92, 0x01, 0x23, 0x00));
            Assert.Equal(new Battery(100, true, false), Parse(Cable, 0x92, 0x01, 0x64, 0x00));
        }

        [Fact]
        public void ChargedOnTheCable()
        {
            Assert.Equal(new Battery(100, false, true), Parse(Cable, 0x92, 0x02, 0x64, 0x00));
        }

        [Fact]
        public void LevelNotKnownRightAfterGoingOnOrOffTheCable()
        {
            Assert.Equal(new Battery(null, true, false), Parse(Cable, 0x92, 0x01, 0xFF, 0x00));
            Assert.Equal(new Battery(null, false, false), Parse(Wireless, 0xD2, 0x00, 0xFF, 0x00));
        }

        [Fact]
        public void AsleepOrOffWhenTheReplyIsNotTheEcho()
        {
            Assert.Null(Parse(Wireless, 0x40, 0xFF, 0x00, 0x00));
            Assert.Null(Parse(Cable, 0x11, 0x00));
        }

        [Fact]
        public void TooShortIsNoReading()
        {
            Assert.Null(SteelSeriesProvider.Parse(new byte[] { 0x00, 0xD2, 0x00 }, Protocol.AeroxGen2, Wireless));
        }

        [Fact]
        public void FirstAeroxStillReadsItsOwnWay()
        {
            // Bit 7 is charging, the rest is (level / 5) + 1: 0x95 = charging, 100%
            Assert.Equal(new Battery(100, true, false),
                SteelSeriesProvider.Parse(new byte[] { 0x00, 0xD2, 0x95 }, Protocol.Aerox, Wireless));
            Assert.Equal(new Battery(50, false, false),
                SteelSeriesProvider.Parse(new byte[] { 0x00, 0x92, 0x0B }, Protocol.Aerox, Cable));
        }
    }
}
