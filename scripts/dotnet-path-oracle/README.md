# Linux path reference results

This program generates `tests/fixtures/dotnet-linux-paths.json` with .NET path
methods and C# literals. It does not call the Rust implementation. The checked
fixture was generated on Linux with SDK 8.0.100 and runtime .NET 8.0.0.
Normal application builds and Rust tests do not require .NET.

To regenerate the fixture, use that SDK version from the repository root:

```sh
dotnet build scripts/dotnet-path-oracle/Oracle.csproj --nologo
dotnet scripts/dotnet-path-oracle/bin/Debug/net8.0/Oracle.dll > tests/fixtures/dotnet-linux-paths.json
cargo test --locked matches_recorded_dotnet_linux_results
```

`NuGet.Config` clears package sources. The project uses the SDK's installed
framework and needs no package download. Review fixture changes when upgrading
the SDK or changing cases. The runtime version and case count are checked.
The fixture proves behavior only for its cases. Windows path handling, size
limits, and unsupported string forms have separate Rust tests.
