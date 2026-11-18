# Does not change PowerShell execution policy or export private keys.
param([Parameter(Mandatory=$true)][string]$AppPath)
$ErrorActionPreference='Stop'
if (!(Test-Path -LiteralPath $AppPath -PathType Leaf)) { throw 'Built app not found' }
if (!$env:DESKLINK_SIGNTOOL) { throw 'Set DESKLINK_SIGNTOOL to Windows SDK signtool.exe' }
$signArgs=@('sign','/fd','SHA256','/td','SHA256','/tr','http://timestamp.digicert.com')
if ($env:DESKLINK_SIGNING_DLIB -and $env:DESKLINK_SIGNING_METADATA) {
    $signArgs+=@('/dlib',$env:DESKLINK_SIGNING_DLIB,'/dmdf',$env:DESKLINK_SIGNING_METADATA)
} elseif ($env:DESKLINK_SIGNING_THUMBPRINT) {
    $signArgs+=@('/sha1',$env:DESKLINK_SIGNING_THUMBPRINT)
} else { throw 'Set a code-signing thumbprint or a configured signing-service library/metadata pair' }
& $env:DESKLINK_SIGNTOOL @signArgs $AppPath
if ($LASTEXITCODE -ne 0) { throw 'Publisher signing failed' }
& $env:DESKLINK_SIGNTOOL verify /pa /all /v $AppPath
if ($LASTEXITCODE -ne 0) { throw 'Publisher signature verification failed' }
$signature=Get-AuthenticodeSignature -LiteralPath $AppPath
if ($signature.Status -ne 'Valid' -or !$signature.TimeStamperCertificate) { throw 'A valid timestamped publisher signature is required' }
Get-FileHash -LiteralPath $AppPath -Algorithm SHA256
