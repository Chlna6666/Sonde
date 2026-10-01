namespace Sonde.Tests;

public class DeviceIdTests
{
    [Fact]
    public void GeneratedDeviceIdMatchesServerContract()
    {
        string id = DeviceId.GenerateDeviceId();
        Assert.NotNull(id);
        Assert.True(Guid.TryParse(id, out _));
        Assert.Equal(id, DeviceId.ValidateDeviceId(id));
    }

    [Fact]
    public void MachineDeviceIdIsStableAndValid()
    {
        string id1 = DeviceId.MachineDeviceId();
        Assert.NotNull(id1);
        Assert.Equal(64, id1.Length);
        Assert.Equal(id1, DeviceId.ValidateDeviceId(id1));

        string id2 = DeviceId.MachineDeviceId();
        Assert.Equal(id1, id2);

        string customSaltId = DeviceId.MachineDeviceId("custom-salt");
        Assert.Equal(64, customSaltId.Length);
        Assert.NotEqual(id1, customSaltId);
    }
}
