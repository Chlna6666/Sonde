using System.Net.Http.Headers;
using System.Net.Http.Json;
using System.Text;
using System.Text.Json;

namespace Sonde.Internal;

internal sealed class Transport : IDisposable
{
    private const string IngestPath = "/api/v1/ingest";
    private const long TokenRefreshMarginMs = 60_000;

    private readonly HttpClient _http;
    private readonly string _endpoint;
    private readonly string _apiKey;
    private readonly string _deviceId;
    private readonly SemaphoreSlim _tokenLock = new(1, 1);

    private TokenState? _cachedToken;

    public Transport(string baseUrl, string apiKey, string deviceId, string userAgent, TimeSpan timeout, HttpClient? httpClient = null)
    {
        _endpoint = NormalizeEndpoint(baseUrl);
        _apiKey = apiKey;
        _deviceId = deviceId;

        if (httpClient != null)
        {
            _http = httpClient;
        }
        else
        {
            _http = new HttpClient { Timeout = timeout };
            _http.DefaultRequestHeaders.UserAgent.ParseAdd(userAgent);
        }
    }

    public string Endpoint => _endpoint;
    public string DeviceId => _deviceId;

    public static string NormalizeEndpoint(string baseUrl)
    {
        string trimmed = baseUrl.Trim().TrimEnd('/');
        return trimmed.EndsWith(IngestPath, StringComparison.OrdinalIgnoreCase)
            ? trimmed
            : $"{trimmed}{IngestPath}";
    }

    public async Task<TokenState> GetTokenAsync(CancellationToken cancellationToken = default)
    {
        long now = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds();
        if (_cachedToken is { } token && token.ExpiresAt - now > TokenRefreshMarginMs)
        {
            return token;
        }

        await _tokenLock.WaitAsync(cancellationToken).ConfigureAwait(false);
        try
        {
            now = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds();
            if (_cachedToken is { } existing && existing.ExpiresAt - now > TokenRefreshMarginMs)
            {
                return existing;
            }

            var request = new HttpRequestMessage(HttpMethod.Post, $"{_endpoint}/token")
            {
                Headers = { Authorization = new AuthenticationHeaderValue("Bearer", _apiKey) },
                Content = new StringContent(
                    JsonSerializer.Serialize(new TokenRequest { DeviceId = _deviceId }),
                    Encoding.UTF8,
                    "application/json")
            };

            HttpResponseMessage response;
            try
            {
                response = await _http.SendAsync(request, cancellationToken).ConfigureAwait(false);
            }
            catch (Exception ex)
            {
                throw new SondeException("Failed to request ingest token from Sonde.", ex);
            }

            if (!response.IsSuccessStatusCode)
            {
                string body = await response.Content.ReadAsStringAsync(cancellationToken).ConfigureAwait(false);
                throw new SondeException(response.StatusCode, body);
            }

            var issued = await response.Content.ReadFromJsonAsync<TokenResponse>(cancellationToken: cancellationToken).ConfigureAwait(false)
                ?? throw new SondeException("Empty or invalid token response received from Sonde.");

            if (!string.Equals(issued.SignatureVersion, Signing.SignatureVersion, StringComparison.Ordinal))
            {
                throw new SondeException($"Unsupported Sonde request signature version: {issued.SignatureVersion}");
            }

            _cachedToken = new TokenState(issued.Token, issued.SigningKey, issued.ExpiresAt);
            return _cachedToken;
        }
        finally
        {
            _tokenLock.Release();
        }
    }

    public void InvalidateToken(string staleToken)
    {
        if (_cachedToken?.Token == staleToken)
        {
            _cachedToken = null;
        }
    }

    public async Task HeartbeatAsync(DeviceFacts facts, CancellationToken cancellationToken = default)
    {
        byte[] body = JsonSerializer.SerializeToUtf8Bytes(facts);
        using var response = await SignedPostAsync("/heartbeat", body, cancellationToken).ConfigureAwait(false);
        if (!response.IsSuccessStatusCode)
        {
            string errorBody = await response.Content.ReadAsStringAsync(cancellationToken).ConfigureAwait(false);
            throw new SondeException(response.StatusCode, errorBody);
        }
    }

    public async Task<BatchReceipt> SendBatchAsync<T>(string route, IReadOnlyList<T> items, CancellationToken cancellationToken = default)
    {
        if (items.Count == 0)
        {
            return new BatchReceipt { Accepted = 0 };
        }

        byte[] body = JsonSerializer.SerializeToUtf8Bytes(new BatchEnvelope<T> { Items = items });
        using var response = await SignedPostAsync(route, body, cancellationToken).ConfigureAwait(false);

        if (!response.IsSuccessStatusCode)
        {
            string errorBody = await response.Content.ReadAsStringAsync(cancellationToken).ConfigureAwait(false);
            throw new SondeException(response.StatusCode, errorBody);
        }

        return await response.Content.ReadFromJsonAsync<BatchReceipt>(cancellationToken: cancellationToken).ConfigureAwait(false)
            ?? new BatchReceipt { Accepted = items.Count };
    }

    public async Task<HttpResponseMessage> SignedPostAsync(string route, ReadOnlyMemory<byte> body, CancellationToken cancellationToken = default)
    {
        var auth = await GetTokenAsync(cancellationToken).ConfigureAwait(false);
        var response = await SignedPostOnceAsync(route, body, auth, cancellationToken).ConfigureAwait(false);

        if (response.StatusCode != System.Net.HttpStatusCode.Unauthorized)
        {
            return response;
        }

        response.Dispose();
        InvalidateToken(auth.Token);
        var refreshed = await GetTokenAsync(cancellationToken).ConfigureAwait(false);
        return await SignedPostOnceAsync(route, body, refreshed, cancellationToken).ConfigureAwait(false);
    }

    private async Task<HttpResponseMessage> SignedPostOnceAsync(
        string route,
        ReadOnlyMemory<byte> body,
        TokenState auth,
        CancellationToken cancellationToken)
    {
        long timestampMs = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds();
        string nonce = Guid.NewGuid().ToString();
        string canonicalPath = $"{IngestPath}{route}";
        string signature = Signing.Sign(auth.SigningKey, timestampMs, nonce, "POST", canonicalPath, body.Span);

        var request = new HttpRequestMessage(HttpMethod.Post, $"{_endpoint}{route}")
        {
            Content = new ReadOnlyMemoryContent(body)
            {
                Headers = { ContentType = new MediaTypeHeaderValue("application/json") }
            }
        };

        request.Headers.Authorization = new AuthenticationHeaderValue("Bearer", auth.Token);
        request.Headers.TryAddWithoutValidation("x-sonde-timestamp", timestampMs.ToString());
        request.Headers.TryAddWithoutValidation("x-sonde-nonce", nonce);
        request.Headers.TryAddWithoutValidation("x-sonde-signature", signature);

        try
        {
            return await _http.SendAsync(request, cancellationToken).ConfigureAwait(false);
        }
        catch (Exception ex) when (ex is not SondeException)
        {
            throw new SondeException("HTTP request to Sonde failed.", ex);
        }
    }

    public void Dispose()
    {
        _tokenLock.Dispose();
        _http.Dispose();
    }
}

internal sealed record TokenState(string Token, string SigningKey, long ExpiresAt);
