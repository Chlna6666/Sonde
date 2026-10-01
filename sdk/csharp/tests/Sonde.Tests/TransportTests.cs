using System.Net;
using System.Text.Json;
using Sonde.Internal;

namespace Sonde.Tests;

public class TransportTests
{
    [Theory]
    [InlineData("https://sonde.example.com", "https://sonde.example.com/api/v1/ingest")]
    [InlineData("https://sonde.example.com/", "https://sonde.example.com/api/v1/ingest")]
    [InlineData("https://sonde.example.com/api/v1/ingest", "https://sonde.example.com/api/v1/ingest")]
    [InlineData("https://sonde.example.com/api/v1/ingest/", "https://sonde.example.com/api/v1/ingest")]
    public void NormalizesServerAndIngestUrls(string input, string expected)
    {
        Assert.Equal(expected, Transport.NormalizeEndpoint(input));
    }

    [Fact]
    public async Task AcquiresTokenAndSendsSignedHeartbeat()
    {
        var handler = new MockHttpMessageHandler();
        using var httpClient = new HttpClient(handler);

        using var transport = new Transport(
            "https://sonde.example.com",
            "api_test_key",
            "test_device_1234",
            "sonde-test",
            TimeSpan.FromSeconds(5),
            httpClient);

        var token = await transport.GetTokenAsync();
        Assert.Equal("test_token_abc", token.Token);
        Assert.Equal("test_signing_key_xyz", token.SigningKey);

        var facts = new DeviceFacts
        {
            AppVersion = "1.0.0",
            Os = "windows"
        };
        await transport.HeartbeatAsync(facts);

        Assert.Equal(2, handler.Requests.Count);
        var tokenReq = handler.Requests[0];
        Assert.Equal("https://sonde.example.com/api/v1/ingest/token", tokenReq.RequestUri?.ToString());
        Assert.Equal("Bearer", tokenReq.Headers.Authorization?.Scheme);
        Assert.Equal("api_test_key", tokenReq.Headers.Authorization?.Parameter);

        var heartbeatReq = handler.Requests[1];
        Assert.Equal("https://sonde.example.com/api/v1/ingest/heartbeat", heartbeatReq.RequestUri?.ToString());
        Assert.Equal("Bearer", heartbeatReq.Headers.Authorization?.Scheme);
        Assert.Equal("test_token_abc", heartbeatReq.Headers.Authorization?.Parameter);
        Assert.True(heartbeatReq.Headers.Contains("x-sonde-signature"));
        Assert.True(heartbeatReq.Headers.Contains("x-sonde-timestamp"));
        Assert.True(heartbeatReq.Headers.Contains("x-sonde-nonce"));
    }

    private sealed class MockHttpMessageHandler : HttpMessageHandler
    {
        public List<HttpRequestMessage> Requests { get; } = new();

        protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken)
        {
            Requests.Add(request);

            if (request.RequestUri?.AbsolutePath.EndsWith("/token") == true)
            {
                var tokenResp = new TokenResponse
                {
                    Token = "test_token_abc",
                    SigningKey = "test_signing_key_xyz",
                    ExpiresAt = DateTimeOffset.UtcNow.AddHours(1).ToUnixTimeMilliseconds(),
                    SignatureVersion = "sonde-hmac-sha256-v2"
                };
                return Task.FromResult(new HttpResponseMessage(HttpStatusCode.OK)
                {
                    Content = System.Net.Http.Json.JsonContent.Create(tokenResp)
                });
            }

            if (request.RequestUri?.AbsolutePath.EndsWith("/heartbeat") == true)
            {
                return Task.FromResult(new HttpResponseMessage(HttpStatusCode.OK));
            }

            return Task.FromResult(new HttpResponseMessage(HttpStatusCode.NotFound));
        }
    }
}
