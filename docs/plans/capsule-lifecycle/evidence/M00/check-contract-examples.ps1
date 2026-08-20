$ErrorActionPreference = 'Stop'

$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..\..\..')).Path
$contractRoot = Join-Path $root 'docs\plans\capsule-lifecycle\contracts'
$exampleRoot = Join-Path $root 'docs\plans\capsule-lifecycle\examples'

$pairs = @(
    @('capsule-application-profile-v0.3.schema.json', 'diagram-studio-application-profile.json'),
    @('capsule-instance-profile-v0.3.schema.json', 'diagram-studio-instance-profile.json'),
    @('capsule-data-contract-v0.3.schema.json', 'diagram-studio-data-contract.json'),
    @('capsule-lineage-v0.3.schema.json', 'diagram-studio-lineage-example.json'),
    @('capsule-migration-v0.3.schema.json', 'diagram-studio-migration-v1-to-v2.json'),
    @('upgrade-plan-v1.schema.json', 'diagram-studio-upgrade-plan-same-schema.json')
)

foreach ($pair in $pairs) {
    $schema = Join-Path $contractRoot $pair[0]
    $example = Join-Path $exampleRoot $pair[1]
    $valid = Get-Content -Raw -LiteralPath $example |
        Test-Json -SchemaFile $schema -ErrorAction Stop
    if (-not $valid) {
        throw "Example $($pair[1]) does not conform to $($pair[0])"
    }
    Write-Output "schema: $($pair[1]) -> $($pair[0]): pass"
}

$domainSql = Get-Content -Raw -LiteralPath (Join-Path $root 'examples\diagram-studio\domain.sql')
$tableMatches = [regex]::Matches(
    $domainSql,
    '(?im)^\s*CREATE\s+TABLE\s+(?:IF\s+NOT\s+EXISTS\s+)?"?(?<name>[a-z_][a-z0-9_]*)'
)
$schemaTables = @($tableMatches | ForEach-Object { $_.Groups['name'].Value } | Sort-Object -Unique)
$dataContract = Get-Content -Raw -LiteralPath (Join-Path $exampleRoot 'diagram-studio-data-contract.json') |
    ConvertFrom-Json -Depth 100
$contractTables = @(
    $dataContract.datasets |
        ForEach-Object { $_.tables } |
        ForEach-Object { $_.name } |
        Sort-Object
)

if ($contractTables.Count -ne (@($contractTables | Sort-Object -Unique)).Count) {
    throw 'Diagram Studio data contract classifies a table more than once'
}

$missing = @($schemaTables | Where-Object { $_ -notin $contractTables })
$unknown = @($contractTables | Where-Object { $_ -notin $schemaTables })
if ($missing.Count -ne 0 -or $unknown.Count -ne 0) {
    throw "Diagram Studio table coverage mismatch; missing=$($missing -join ','); unknown=$($unknown -join ',')"
}

Write-Output "coverage: $($schemaTables.Count) domain tables classified exactly once: pass"
