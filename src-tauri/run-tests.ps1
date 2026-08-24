# paxi Rust test runner (Windows).
#
# Problem: tauri links comctl32!TaskDialogIndirect which only exists in
# Common-Controls v6. The main exe gets a manifest from tauri-build, but
# rustc-built test exes have none, so they load the old comctl32 5.82 and
# crash at startup with STATUS_ENTRYPOINT_NOT_FOUND (0xC0000139).
#
# Fix: Windows SxS also loads an external "<exe>.manifest" placed next to
# the exe. We compile tests without running, copy a manifest next to each
# test exe, then run.
#
# Usage: powershell -File run-tests.ps1 [extra cargo test args]

$exes = @()
cargo test --no-run --message-format=json 2>$null | ForEach-Object {
    try {
        $m = $_ | ConvertFrom-Json
        if ($m.reason -eq "compiler-artifact" -and $m.executable -and $m.executable -like "*engine_test*") {
            $exes += $m.executable
        }
    } catch { }
}
if ($exes.Count -eq 0) { throw "test exe not found (compile failed?)" }

$manifest = Join-Path $PSScriptRoot "tests\test.manifest"
foreach ($exe in $exes) {
    Copy-Item $manifest "$exe.manifest" -Force
    Write-Host "manifest -> $exe.manifest"
}

cargo test @args
exit $LASTEXITCODE
