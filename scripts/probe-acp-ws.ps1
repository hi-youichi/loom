param(
  [string]$Url = 'ws://127.0.0.1:3151/acp',
  [string]$Origin = 'http://127.0.0.1:3151'
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Net.WebSockets, System.Net.Http

function Send-Frame([System.Net.WebSockets.WebSocket]$ws, [string]$text) {
  $bytes = [System.Text.Encoding]::UTF8.GetBytes($text)
  $seg = [ArraySegment[byte]]::new($bytes)
  $t = $ws.SendAsync($seg, [System.Net.WebSockets.WebSocketMessageType]::Text, $true, [Threading.CancellationToken]::None)
  $t.Wait(5000) | Out-Null
}

function Recv-Frame([System.Net.WebSockets.WebSocket]$ws) {
  $buf = New-Object byte[] 65536
  $seg = [ArraySegment[byte]]::new($buf)
  $t = $ws.ReceiveAsync($seg, [Threading.CancellationToken]::None)
  if (-not $t.Wait(8000)) { throw 'recv timeout' }
  if ($t.Result.MessageType -eq [System.Net.WebSockets.WebSocketMessageType]::Close) { throw 'server closed ws' }
  return [System.Text.Encoding]::UTF8.GetString($buf, 0, $t.Result.Count)
}

$ws = [System.Net.WebSockets.ClientWebSocket]::new()
$ws.Options.SetRequestHeader('Origin', $Origin)
try {
  $ct = [Threading.CancellationToken]::None
  $ws.ConnectAsync([Uri]$Url, $ct).Wait(8000) | Out-Null
  Write-Host "[probe] connected: $Url"

  $init = '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1,"clientCapabilities":{"fs":{"readTextFile":false,"writeTextFile":false}}}}'
  Send-Frame $ws $init
  $resp = Recv-Frame $ws
  Write-Host "[probe] initialize -> $($resp.Substring(0, [Math]::Min(300, $resp.Length)))"
  if ($resp -match '"error"') { exit 2 }
  if ($resp -match '"result"') { Write-Host '[probe] OK: gate would pass (dev mode / authenticated)'; exit 0 }
  exit 1
}
catch {
  Write-Host "[probe] FAILED: $($_.Exception.Message)"
  exit 3
}
finally {
  $ws.Dispose()
}
