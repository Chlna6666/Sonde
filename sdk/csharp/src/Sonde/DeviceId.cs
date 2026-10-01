using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Runtime.Versioning;
using System.Security.Cryptography;
using System.Text;

namespace Sonde;

/// <summary>
/// Provides utilities for generating and deriving tamper-resistant pseudonymous device identifiers.
/// </summary>
public static class DeviceId
{
    private const int DeviceIdMinBytes = 4;
    private const int DeviceIdMaxBytes = 128;
    private const string DefaultMachineSalt = "sonde-machine-id-v1";

    /// <summary>
    /// Generates a high-entropy random UUID v4 identifier.
    /// </summary>
    public static string GenerateDeviceId() => Guid.NewGuid().ToString();

    /// <summary>
    /// Derives a stable, tamper-resistant pseudonymous device identifier from the host machine in-memory.
    /// It computes a salted SHA-256 hash of the operating system or hardware machine identifier
    /// (e.g., Windows MachineGuid, Linux /etc/machine-id, or macOS IOPlatformUUID) and does NOT write any files to disk.
    /// </summary>
    /// <param name="salt">Optional application salt to prevent cross-app device correlation.</param>
    /// <returns>A 64-character lowercase hexadecimal device identifier.</returns>
    /// <exception cref="InvalidOperationException">Thrown when the machine identifier cannot be detected.</exception>
    public static string MachineDeviceId(string? salt = null)
    {
        string? raw = GetPlatformMachineCode();
        if (string.IsNullOrWhiteSpace(raw))
        {
            throw new InvalidOperationException("Failed to detect platform machine identifier.");
        }

        string effectiveSalt = salt ?? DefaultMachineSalt;
        byte[] payload = Encoding.UTF8.GetBytes($"{effectiveSalt}:{raw}");
        byte[] hash = SHA256.HashData(payload);
        return Convert.ToHexStringLower(hash);
    }

    internal static string ValidateDeviceId(string value)
    {
        string trimmed = value.Trim();
        if (trimmed.Length < DeviceIdMinBytes || trimmed.Length > DeviceIdMaxBytes)
        {
            throw new ArgumentException($"Device ID must be between {DeviceIdMinBytes} and {DeviceIdMaxBytes} bytes.", nameof(value));
        }

        foreach (char c in trimmed)
        {
            if (!char.IsAsciiLetterOrDigit(c) && c != '-' && c != '_' && c != '.' && c != ':')
            {
                throw new ArgumentException("Device ID must contain only ASCII letters, digits, '-', '_', '.', or ':'.", nameof(value));
            }
        }

        return trimmed;
    }

    private static string? GetPlatformMachineCode()
    {
        if (RuntimeInformation.IsOSPlatform(OSPlatform.Windows))
        {
            return GetWindowsMachineGuid();
        }

        if (RuntimeInformation.IsOSPlatform(OSPlatform.Linux))
        {
            return GetLinuxMachineId();
        }

        if (RuntimeInformation.IsOSPlatform(OSPlatform.OSX))
        {
            return GetMacPlatformUuid();
        }

        return null;
    }

    [SupportedOSPlatform("windows")]
    private static string? GetWindowsMachineGuid()
    {
        try
        {
            using var key = Microsoft.Win32.Registry.LocalMachine.OpenSubKey(@"SOFTWARE\Microsoft\Cryptography");
            if (key?.GetValue("MachineGuid") is string guid && !string.IsNullOrWhiteSpace(guid))
            {
                return $"win:{guid.Trim()}";
            }
        }
        catch
        {
            // Fall through
        }

        return null;
    }

    private static string? GetLinuxMachineId()
    {
        try
        {
            if (File.Exists("/etc/machine-id"))
            {
                string id = File.ReadAllText("/etc/machine-id").Trim();
                if (!string.IsNullOrEmpty(id))
                {
                    return $"linux:{id}";
                }
            }

            if (File.Exists("/var/lib/dbus/machine-id"))
            {
                string id = File.ReadAllText("/var/lib/dbus/machine-id").Trim();
                if (!string.IsNullOrEmpty(id))
                {
                    return $"linux:{id}";
                }
            }
        }
        catch
        {
            // Fall through
        }

        return null;
    }

    private static string? GetMacPlatformUuid()
    {
        try
        {
            using var process = new Process
            {
                StartInfo = new ProcessStartInfo
                {
                    FileName = "ioreg",
                    Arguments = "-rd1 -c IOPlatformExpertDevice",
                    RedirectStandardOutput = true,
                    UseShellExecute = false,
                    CreateNoWindow = true
                }
            };

            process.Start();
            string output = process.StandardOutput.ReadToEnd();
            process.WaitForExit();

            if (process.ExitCode == 0)
            {
                foreach (string line in output.Split('\n'))
                {
                    if (line.Contains("IOPlatformUUID"))
                    {
                        int equalsIdx = line.IndexOf('=');
                        if (equalsIdx >= 0)
                        {
                            string uuid = line[(equalsIdx + 1)..].Trim().Trim('"').Trim();
                            if (!string.IsNullOrEmpty(uuid))
                            {
                                return $"macos:{uuid}";
                            }
                        }
                    }
                }
            }
        }
        catch
        {
            // Fall through
        }

        return null;
    }
}
