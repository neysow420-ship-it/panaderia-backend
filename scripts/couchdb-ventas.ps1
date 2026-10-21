# Crea las bases y sube la vista e índice de ventas a CouchDB (Windows / PowerShell).
# Uso (desde la raíz del repo):  .\scripts\couchdb-ventas.ps1 [-Prueba]
#   -Prueba  además crea 2 productos de ejemplo en `catalogo` para probar POST /ventas.
# Lee COUCHDB_URL, COUCHDB_USER y COUCHDB_PASSWORD del .env.
param([switch]$Prueba)
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

$envVars = @{}
Get-Content .env | Where-Object { $_ -match '^\s*([A-Z_]+)\s*=\s*(.*)$' } | ForEach-Object {
    $envVars[$Matches[1]] = $Matches[2].Trim('"')
}
$url = $envVars["COUCHDB_URL"]
$par = "$($envVars['COUCHDB_USER']):$($envVars['COUCHDB_PASSWORD'])"
$headers = @{ Authorization = "Basic " + [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($par)) }

function Couch($metodo, $ruta, $cuerpo) {
    try {
        $r = Invoke-RestMethod -Method $metodo -Uri "$url/$ruta" -Headers $headers `
            -ContentType "application/json; charset=utf-8" -Body $cuerpo
        Write-Host "  OK $metodo $ruta"
        return $r
    } catch {
        Write-Host "  $metodo $ruta -> $($_.Exception.Response.StatusCode.value__) (412 = ya existía)"
    }
}

foreach ($bd in "panaderia", "catalogo") { Couch PUT $bd $null | Out-Null }

$doc = Get-Content infra/couchdb/ventas/_design_ventas.json -Raw | ConvertFrom-Json
try {
    $actual = Invoke-RestMethod -Uri "$url/panaderia/_design/ventas" -Headers $headers
    $doc | Add-Member -NotePropertyName _rev -NotePropertyValue $actual._rev
} catch { }
Couch PUT "panaderia/_design/ventas" ($doc | ConvertTo-Json -Depth 10) | Out-Null

Couch POST "panaderia/_index" (Get-Content infra/couchdb/ventas/indice_ventas_fecha.json -Raw) | Out-Null

if ($Prueba) {
    Couch PUT "catalogo/producto:pan-frances" '{"tipo":"producto","nombre":"Pan francés","precio_centavos":10,"vida_util_dias":1,"activo":true}' | Out-Null
    Couch PUT "catalogo/producto:donas" '{"tipo":"producto","nombre":"Donas","precio_centavos":35,"vida_util_dias":2,"activo":true}' | Out-Null
}
