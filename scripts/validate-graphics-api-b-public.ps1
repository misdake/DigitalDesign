param(
    [Parameter(Mandatory=$true)][string]$InputDirectory,
    [Parameter(Mandatory=$true)][string]$OutputDirectory
)
$ErrorActionPreference='Stop'
$repo=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$output=[IO.Path]::GetFullPath((Join-Path $repo $OutputDirectory))
$target=[IO.Path]::GetFullPath((Join-Path $repo 'target')) + [IO.Path]::DirectorySeparatorChar
if (!$output.StartsWith($target,[StringComparison]::OrdinalIgnoreCase)) { throw 'Output must be inside this tree target' }
if (Test-Path -LiteralPath $output) { throw 'Use a fresh output directory; prior evidence is preserved' }
New-Item -ItemType Directory -Path $output | Out-Null
foreach($name in @('manifest.json','sources.zip')) {
    Copy-Item -LiteralPath (Join-Path $InputDirectory $name) -Destination (Join-Path $output $name)
}
$manifest=Get-Content -Raw -LiteralPath (Join-Path $output 'manifest.json') | ConvertFrom-Json
if ((Get-FileHash -LiteralPath (Join-Path $output 'sources.zip')).Hash.ToLowerInvariant() -ne $manifest.zip_sha256) { throw 'Input ZIP identity mismatch' }
$source=Join-Path $output 'source'
Expand-Archive -LiteralPath (Join-Path $output 'sources.zip') -DestinationPath $source
foreach($file in $manifest.files) {
    if ((Get-FileHash -LiteralPath (Join-Path $source $file.path)).Hash.ToLowerInvariant() -ne $file.sha256) { throw "Input mismatch: $($file.path)" }
}
$package=Join-Path $repo 'systems/cpu-v3-tang-nano-20k'
$system=Join-Path $source 'systems/cpu-v3-tang-nano-20k/build_support/rcc_modules.rs'
# Freeze the checker and its source fixture as well as the production inputs.
$checkerFiles=@('compiler/graphics_api_b/public.rs','fixtures/graphics_api_b_public.rs')
$checkerIdentity=@()
foreach($relative in $checkerFiles) {
    $original=Join-Path (Join-Path $package 'tests') $relative
    $copy=Join-Path (Join-Path $output 'checker') $relative
    New-Item -ItemType Directory -Path (Split-Path $copy -Parent) -Force | Out-Null
    Copy-Item -LiteralPath $original -Destination $copy
    $hash=(Get-FileHash -LiteralPath $copy).Hash.ToLowerInvariant()
    if ($hash -ne (Get-FileHash -LiteralPath $original).Hash.ToLowerInvariant()) { throw 'Checker snapshot mismatch' }
    $checkerIdentity+=@{path=('checker/'+$relative);sha256=$hash}
}
$identity=[ordered]@{
    source_commit=(& git -C $repo rev-parse HEAD).Trim()
    accepted_dev=(& git -C $repo rev-parse code-v0.3-dev).Trim()
    input_manifest_sha256=(Get-FileHash -LiteralPath (Join-Path $output 'manifest.json')).Hash.ToLowerInvariant()
    script_sha256=(Get-FileHash -LiteralPath $PSCommandPath).Hash.ToLowerInvariant()
    checker_files=$checkerIdentity
    worktree_files=@(& git -C $repo diff --name-only HEAD | ForEach-Object {
        $path=Join-Path $repo $_
        $hash=if (Test-Path -LiteralPath $path -PathType Leaf) { (Get-FileHash -LiteralPath $path).Hash.ToLowerInvariant() } else { $null }
        @{path=$_;sha256=$hash}
    })
    scope='Recording only; capture/dump and final A caller composition require their own execution'
}
$identity | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $output 'run-identity.json') -Encoding utf8NoBOM
$checker=Join-Path $output 'checker/compiler/graphics_api_b/public.rs'
$main=@'
#[path = "@SYSTEM@"]
#[allow(dead_code)]
mod system;
#[path = "@CHECKER@"]
mod checker;
fn main() {
    let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let sources=root.join("source/systems/cpu-v3-tang-nano-20k/rcc");
    checker::run(|label,source,options,folder| {
        let local=folder.join("application");std::fs::create_dir_all(&local).unwrap();
        std::fs::write(local.join("app.rs"),source).unwrap();
        system::compile_explicit_gpu_program_checked(label,source,options,&local,&sources,"",
            &system::RendererCaptureConfiguration::with_default_hook(),
            &rcc::frontend::rustc_check::RustcCheck::new(folder),&mut |_| {})
    },&root.join("results"));
}
'@
$main=$main.Replace('@SYSTEM@',$system.Replace('\','/')).Replace('@CHECKER@',$checker.Replace('\','/'))
Set-Content -LiteralPath (Join-Path $output 'main.rs') -Value $main -Encoding utf8NoBOM
$cargo=@'
[package]
name="graphics-api-b-public-qualification"
version="0.0.0"
edition="2021"
[workspace]
[dependencies]
rcc={path="@REPO@/compiler/rcc"}
cpu-v3={path="@REPO@/ip/cpu-v3"}
serde_json="1"
[[bin]]
name="graphics-api-b-public-qualification"
path="main.rs"
'@
Set-Content -LiteralPath (Join-Path $output 'Cargo.toml') -Value $cargo.Replace('@REPO@',$repo.Replace('\','/')) -Encoding utf8NoBOM
$env:CARGO_TARGET_DIR=Join-Path $repo 'target/cargo'
Push-Location $repo
try {
    & "$repo/scripts/run-cargo.ps1" -Subcommand run -Label ('api-b-public-'+(Split-Path $output -Leaf)) -CargoArgs @('--manifest-path',(Join-Path $output 'Cargo.toml'))
    if ($LASTEXITCODE -ne 0) { throw 'Public qualification failed; see bounded run log' }
} finally { Pop-Location }
