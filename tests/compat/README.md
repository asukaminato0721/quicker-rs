# Shared action compatibility checks

The tool uses the v1 API found in the supplied Quicker 1.45.5 MSI.
It does not claim support for every Quicker version or action.

## MSI evidence

The inspection extracted managed assemblies without running the installer.
`dnfile` and `dncil` read the assembly metadata and method bodies.

| Artifact | SHA-256 |
| --- | --- |
| Quicker.x64.1.45.5.0.msi | `97418d96c00e25c8811b3c5fe3d7df6bb07a9b7be0849a6a1e41d9a1b8896a95` |
| Quicker.exe | `ac815c9280dee1ff2e476da5f6035a0451d07d3c0c4e12a02f0861f19fd4d8bd` |
| Quicker.Common.dll | `838e949d15b376c087b2bf0d00bf14f3c6c1b0e122a06ffea4213c800f71a594` |

`<DownloadSharedActionAsync>d__32.MoveNext` in Quicker.exe contains this route:

```text
https://api.getquicker.net/api/SharedAction/Download
    ?id={GUID}&revision={optional integer}&softVersion={version}&forPreview={boolean}
```

The method body starts at RVA `0x3d3384` in this assembly.
The API client initializer contains `https://api.getquicker.net` and `/api`.
`CreateClient` at RVA `0x282a9c` sets `DefaultRequestHeaders.Authorization`
with the `Bearer` scheme. The tool reads the token from `QUICKER_API_TOKEN`.

The response type is `ApiResult<SharedActionDto>` from Quicker.Common.dll.
`ApiResult<T>` defines `IsSuccess`, `ErrorCode`, `Message`, `Errors`, and `Data`.
`SharedActionDto` defines `Id`, `Revision`, `ActionType`, `Title`, and workflow
`Data`, plus other action metadata. The tool checks the ID and requested revision.
It preserves the HTTP response separately from the extracted action document.
The decoder accepts PascalCase and camelCase envelope fields.

The anonymous official request returned HTTP 401 on 2026-10-06.
No authenticated success response was available for this session.
Offline tests check response decoding against the MSI-defined structure.
They do not establish a successful authenticated download.

## Real public export

The [public export registry](public-exports.json) maps one shared ID to an
[author-published export](https://github.com/AlexShyXie/myCitaviMacros/tree/50fd22f27e4c752459ef9fe0dc1c96d490cef81d/Quicker%E5%8A%A8%E4%BD%9C%E9%A2%84%E8%AE%BE).
The registry records the exact repository commit and response hash.
The export's `SharedActionId` matches the requested ID.
The repository commit is not an official action revision.

Run the complete download and check:

```sh
cargo build --locked
python3 scripts/check-shared-action.py 6803b583-78f7-400d-a4c1-08de12ec7091 --public-export
```

On 2026-10-06, the request downloaded `Ref->Ob` and matched the recorded SHA-256.
All preservation checks passed. The runtime report returned `blocked` and exit code 1.
It identified `sys:subprogram`, `sys:waitClipboardChange`, and
`sys:activateProcessMainWindow` as missing runners. The tool did not execute the action.

Five other downloaded exports passed the same preservation checks.
These files were `obPDF->Cit`, `xID->CITAVI`, `KnowPDF++`, `AnnoPDF++`, and
[用QuickLook预览文件](https://www.getquicker.net/Common/Topics/ViewTopic/24595).
All five also contain missing runners. Passing preservation checks does not
establish execution compatibility with Citavi, QuickLook, or Windows APIs.

## Automated checks

```sh
cargo test --all-targets --locked
cargo build --locked
python3 scripts/test-check-shared-action.py
```

The Python tests do not need network access. They check authentication failures,
ID and revision mismatches, response shapes, redirects, size limits, and token handling.
They also pass downloaded response fixtures and local files to the actual Rust checker.
The Rust tests check preservation and runtime diagnostics.

The checker reports static evidence only. It does not validate all expressions,
application dependencies, permissions, platform behavior, or unknown option semantics.
Reports always set `runtime.executed` to `false`.

## Control flow evidence

Quicker.exe delegates its loop runners to managed closure methods. The repeat
body at RVA `0x3f1094` writes `count` before it evaluates `stopCondition`.
It reads the iteration limit and start index once. It checks the stop condition
on each iteration. A count of -1 selects an unlimited loop.
The sequential each body at RVA `0x3efbb8` writes the item and zero-based index
before child steps. Both runners consume the nearest loop's break and continue.
The if body at RVA `0x3f0ebc` selects either branch. The simpleIf body at
RVA `0x3f18b4` only executes its true branch.

Runtime tests check these orders with controlled workflows. They also check
nested loops, typed items, stop propagation, invalid counters, and cancellation.
These tests do not execute the downloaded actions' Windows applications.
