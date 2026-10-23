# Crea las bases y sube la vista de productos a CouchDB (Windows / PowerShell).
# Uso (desde la raíz del repo):  .\scripts\couchdb-productos.ps1 [-Prueba]
#   -Prueba  además siembra los 5 productos y el documento `config:vigente`
#            en `panaderia`, para probar /productos y /config.
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
        Write-Host "  $metodo $ruta -> $($_.Exception.Response.StatusCode.value__) (412 o 409 = ya existía)"
    }
}

foreach ($bd in "panaderia", "catalogo") { Couch PUT $bd $null | Out-Null }

# La vista vive en `catalogo`. Si el design doc ya existe, se reemplaza usando su _rev.
$doc = Get-Content infra/couchdb/productos/_design_productos.json -Raw | ConvertFrom-Json
try {
    $actual = Invoke-RestMethod -Uri "$url/catalogo/_design/productos" -Headers $headers
    $doc | Add-Member -NotePropertyName _rev -NotePropertyValue $actual._rev
} catch { }
Couch PUT "catalogo/_design/productos" ($doc | ConvertTo-Json -Depth 10) | Out-Null

if ($Prueba) {
    python scripts/sembrar_catalogo.py
}