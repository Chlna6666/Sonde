namespace Sonde;

public sealed class RetryPolicy
{
    public int MaxRetries { get; set; } = 5;
    public TimeSpan InitialBackoff { get; set; } = TimeSpan.FromMilliseconds(250);
    public TimeSpan MaxBackoff { get; set; } = TimeSpan.FromSeconds(15);
}

public sealed class DeliveryOptions
{
    public int QueueCapacity { get; set; } = 4096;
    public int MaxBatchItems { get; set; } = 256;
    public TimeSpan FlushInterval { get; set; } = TimeSpan.FromSeconds(1);
    public RetryPolicy Retry { get; set; } = new();
}

public sealed class SondeOptions
{
    public required string BaseUrl { get; set; }
    public required string ApiKey { get; set; }
    public string? DeviceId { get; set; }
    public string UserAgent { get; set; } = "sonde-csharp-sdk/0.1.10";
    public DeviceFacts Facts { get; set; } = DeviceFacts.CreatePlatformDefaults();
    public TimeSpan? HeartbeatInterval { get; set; } = TimeSpan.FromSeconds(60);
    public TimeSpan RequestTimeout { get; set; } = TimeSpan.FromSeconds(10);
    public HttpClient? HttpClient { get; set; }
    public DeliveryOptions Delivery { get; set; } = new();
}
