// Sample git.exe child processes: check for visible windows (console flash detection).
$deadline = (Get-Date).AddSeconds(50)
$seen = 0; $withWindow = 0
while ((Get-Date) -lt $deadline) {
  $g = Get-Process git -ErrorAction SilentlyContinue
  foreach ($p in $g) {
    $seen++
    if ($p.MainWindowHandle -ne 0) { $withWindow++; "$((Get-Date).ToString('HH:mm:ss.fff')) git pid=$($p.Id) WINDOW=0x$($p.MainWindowHandle)" }
  }
  Start-Sleep -Milliseconds 120
}
"sampled=$seen withVisibleWindow=$withWindow"
