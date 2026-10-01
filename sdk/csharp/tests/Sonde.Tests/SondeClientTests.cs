using System.Net;
using System.Net.Http.Json;
using System.Text.Json;

namespace Sonde.Tests;

public class SondeClientTests
{
    [Fact]
    public async Task ClientBatchesAndFlushesEvents()
    {
        var handler = new MockSondeServerHandler();
        using var httpClient = new HttpClient(handler);

        var options = new SondeOptions
        {
            BaseUrl = "https://sonde.example.com",
            ApiKey = "sonde_test_key_123",
            DeviceId = "test_client_device_999",
            Facts = new DeviceFacts { AppVersion = "2.0.0", Os = "windows" },
            HttpClient = httpClient,
            Delivery = new DeliveryOptions
            {
                QueueCapacity = 100,
                MaxBatchItems = 10,
                FlushInterval = TimeSpan.FromSeconds(60)
            }
        };

        await using var client = await SondeClient.ConnectAsync(options);

        client.TrackEvent(new Event("button_click").WithAttribute("buttonId", "submit"));
        client.TrackMetric(Metric.Counter("clicks", 1));
        client.TrackLog(new LogEntry(LogLevel.Info, "user logged in"));
        client.TrackError(new ErrorEvent("AppError", "something failed"));

        // Flush all queues
        await client.FlushAsync();

        // Ensure token, heartbeat, and batch ingest requests were received
        Assert.Contains(handler.Requests, r => r.RequestUri?.AbsolutePath.EndsWith("/token") == true);
        Assert.Contains(handler.Requests, r => r.RequestUri?.AbsolutePath.EndsWith("/heartbeat") == true);
        Assert.Contains(handler.Requests, r => r.RequestUri?.AbsolutePath.EndsWith("/events") == true);
        Assert.Contains(handler.Requests, r => r.RequestUri?.AbsolutePath.EndsWith("/metrics") == true);
        Assert.Contains(handler.Requests, r => r.RequestUri?.AbsolutePath.EndsWith("/logs") == true);
        Assert.Contains(handler.Requests, r => r.RequestUri?.AbsolutePath.EndsWith("/errors") == true);
    }

    private sealed class MockSondeServerHandler : HttpMessageHandler
    {
        public List<HttpRequestMessage> Requests { get; } = new();

        protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken)
        {
            Requests.Add(request);

            if (request.RequestUri?.AbsolutePath.EndsWith("/token") == true)
            {
                var tokenResp = new TokenResponse
                {
                    Token = "token_val",
                    SigningKey = "signing_key_val",
                    ExpiresAt = DateTimeOffset.UtcNow.AddHours(2).ToUnixTimeMilliseconds(),
                    SignatureVersion = "sonde-hmac-sha256-v2"
                };
                return Task.FromResult(new HttpResponseMessage(HttpStatusCode.OK)
                {
                    Content = JsonContent.Create(tokenResp)
                });
            }

            if (request.RequestUri?.AbsolutePath.EndsWith("/heartbeat") == true)
            {
                return Task.FromResult(new HttpResponseMessage(HttpStatusCode.OK));
            }

            if (request.RequestUri?.AbsolutePath.Contains("/events") == true ||
                request.RequestUri?.AbsolutePath.Contains("/metrics") == true ||
                request.RequestUri?.AbsolutePath.Contains("/logs") == true ||
                request.RequestUri?.AbsolutePath.Contains("/errors") == true)
            {
                var receipt = new BatchReceipt { Accepted = 1 };
                return Task.FromResult(new HttpResponseMessage(HttpStatusCode.OK)
                {
                    Content = JsonContent.Create(receipt)
                });
            }

            return Task.FromResult(new HttpResponseMessage(HttpStatusCode.NotFound));
        }
    }
}
