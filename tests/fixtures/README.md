The action documents are authored, minimal Quicker-format fixtures for parser and interpreter
regression tests. They replace references to absent `sample/` exports from the
prototype; they are not copies of those original exports. The formula image
workflow uses mocked dialogs, downloads, and clipboard operations in unit tests.

Coverage includes legacy launch and key macro documents, nested conditions,
variable bindings, clipboard formats, regex extraction, image conversion, and
action state. These tests do not prove compatibility with every Quicker action.

`dotnet-linux-paths.json` is a separate reference dataset. It contains 234
results from .NET 8.0.0 on Linux, generated with SDK 8.0.100. The inputs cover
path components, nulls, dotfiles, extensions, separator runs, combination, and
C# string literals. Rust tests compare the interpreter against these results.
The generator is in [scripts/dotnet-path-oracle](../../scripts/dotnet-path-oracle/).
The application and normal tests do not require .NET.
