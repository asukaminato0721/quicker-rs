using System.Text.Json;
var cases = new List<object>();
void Add(string expression, object? value) => cases.Add(new { expression = "$= " + expression, expected = value });
foreach (string? p in new string?[] { null, "", "/", "//", "///", "a", "a/", "a//", "a/b", "/tmp/中文.tar.gz", "/tmp/.config", "file.", "..", ".", "a/../b", "./file", " a / file.txt ", "a\nb.txt", "/tmp//a///b", "😀/中文.txt" }) {
    string q = JsonSerializer.Serialize(p);
    Add($"Path.GetDirectoryName({q})", Path.GetDirectoryName(p));
    Add($"Path.GetFileName({q})", Path.GetFileName(p));
    Add($"Path.GetFileNameWithoutExtension({q})", Path.GetFileNameWithoutExtension(p));
    Add($"Path.GetExtension({q})", Path.GetExtension(p));
    Add($"Path.GetPathRoot({q})", Path.GetPathRoot(p));
    Add($"Path.HasExtension({q})", Path.HasExtension(p));
    Add($"Path.IsPathRooted({q})", Path.IsPathRooted(p));
    foreach (string? e in new string?[] { null, "", "txt", ".tar.gz" })
        Add($"Path.ChangeExtension({q}, {JsonSerializer.Serialize(e)})", Path.ChangeExtension(p, e));
}
foreach (var parts in new string[][] { Array.Empty<string>(), new[]{"a"}, new[]{"a","b"}, new[]{"a","/b","c"}, new[]{"/tmp//","file"}, new[]{"a","../b"}, new[]{"",""}, new[]{"a","","b","c","d"} }) {
    Add($"Path.Combine({string.Join(", ", parts.Select(p => JsonSerializer.Serialize(p)))})", Path.Combine(parts));
}
Add("@\"\\\"", @"\");
Add("@\"a\"\"b{missing}\"", @"a""b{missing}");
Add("@\"a\r\n中\"", "a\r\n中");
Add("\"\\x12345\"", "\x12345");
Add("\"\\U0001F600\"", "\U0001F600");
Add("\"\\ud83d\\ude00\"", "\ud83d\ude00");
Console.WriteLine(JsonSerializer.Serialize(new { runtime = System.Runtime.InteropServices.RuntimeInformation.FrameworkDescription, cases }, new JsonSerializerOptions { WriteIndented = true }));
