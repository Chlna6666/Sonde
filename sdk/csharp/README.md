# Sonde .NET 10 SDK (`Sonde.SDK`)

Official .NET 10 SDK for [Sonde](https://github.com/Chlna6666/Sonde) telemetry ingest.

## Features

- **.NET 10 Target (`net10.0`)**: Built on modern C# with zero external package dependencies.
- **In-Memory Machine Device ID**: Hardware/OS-derived tamper-resistant pseudonymous ID (Windows `MachineGuid`, Linux `/etc/machine-id`, macOS `IOPlatformUUID`). No plain-text files written to disk.
- **HMAC-SHA256 Request Signing**: Full `sonde-hmac-sha256-v2` ingest protocol implementation with token lifecycle caching.
- **Background Buffered Queues**: High-performance `System.Threading.Channels` actor queues with automatic batching, exponential backoff retries, and reliable flushing.
- **Telemetry Streams**: Full support for Events, Metrics (Counter, Gauge, Histogram), Logs, and Error Events.

## Quick Start

```csharp
using Sonde;

// Connect with in-memory machine identifier
await using var client = await SondeClient.FromMachineAsync(
    baseUrl: "https://sonde.example.com",
    apiKey: "sonde_your_api_key_here",
    salt: "my-app-name" // optional
);

// 1. Track Events
client.TrackEvent(new Event("app_startup")
    .WithAppVersion("1.0.0")
    .WithAttribute("channel", "stable"));

// 2. Track Metrics
client.TrackMetric(Metric.Counter("button_clicks", 1));
client.TrackMetric(Metric.Gauge("cpu_usage", 14.5));

// 3. Track Logs
client.TrackLog(new LogEntry(LogLevel.Info, "Application initialized successfully")
    .WithLogger("main"));

// 4. Track Errors
try
{
    // ...
}
catch (Exception ex)
{
    client.TrackError(ErrorEvent.FromException(ex, handled: true));
}

// Flush all buffered queues before exit
await client.FlushAsync();
```

## Advanced Options

```csharp
var options = new SondeOptions
{
    BaseUrl = "https://sonde.example.com",
    ApiKey = "sonde_your_api_key_here",
    // Optional: override device ID if you have your own identifier
    DeviceId = DeviceId.MachineDeviceId("my-app-salt"),
    HeartbeatInterval = TimeSpan.FromSeconds(60),
    RequestTimeout = TimeSpan.FromSeconds(10),
    Delivery = new DeliveryOptions
    {
        QueueCapacity = 4096,
        MaxBatchItems = 256,
        FlushInterval = TimeSpan.FromSeconds(1),
        Retry = new RetryPolicy
        {
            MaxRetries = 5,
            InitialBackoff = TimeSpan.FromMilliseconds(250),
            MaxBackoff = TimeSpan.FromSeconds(15)
        }
    }
};

await using var client = await SondeClient.ConnectAsync(options);
```

## Running Tests

```bash
dotnet test sdk/csharp/Sonde.slnx
```
