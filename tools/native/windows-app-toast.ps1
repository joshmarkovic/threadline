function Register-AppToastFixture {
    Add-Type -Path (Join-Path $PSScriptRoot 'ThreadlineToastShortcut.cs')
    $token = [guid]::NewGuid().ToString('N')
    $classId = [guid]::ParseExact($token, 'N')
    $className = $classId.ToString('B')
    $identity = 'Threadline.Feasibility.' + $token
    $display = 'Threadline Toast Fixture ' + $token
    $programs = [Environment]::GetFolderPath([Environment+SpecialFolder]::Programs)
    if (![IO.Directory]::Exists($programs)) { Stop-Blocked 'The current user has no Programs known folder for the isolated shortcut.' }
    $command = '"' + $script:Binary + '" --notification-com-server'
    $fixture = [pscustomobject]@{
        Token = $token; ClassId = $classId; ClassName = $className; Identity = $identity; Display = $display
        Link = Join-Path $programs ($display + '.lnk'); Command = $command
        ClassCreated = $false
    }
    $script:toastFixture = $fixture
    $classes = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Software\Classes\CLSID', $true)
    if ($null -eq $classes) { Stop-Blocked 'The per-user COM registration directory is not available.' }
    try {
        $existing = $classes.OpenSubKey($className)
        if ($null -ne $existing) {
            $existing.Dispose()
            Stop-Blocked 'The unique test class already has an unrelated registration.'
        }
        $entry = $classes.CreateSubKey($className)
        $fixture.ClassCreated = $true
        try {
            $entry.SetValue('ThreadlineFeasibilityOwner', $token, [Microsoft.Win32.RegistryValueKind]::String)
            $server = $entry.CreateSubKey('LocalServer32')
            try {
                $server.SetValue('', $command, [Microsoft.Win32.RegistryValueKind]::String)
                if ($server.GetValue('') -cne $command) { Stop-Blocked 'The isolated local COM server did not read back.' }
            } finally { $server.Dispose() }
        } finally { $entry.Dispose() }
    } finally { $classes.Dispose() }

    [ThreadlineToastShortcut]::CreateOwnedLinkWithActivator($fixture.Link, $script:Binary, $identity, $classId)
    $linkMatches = [ThreadlineToastShortcut]::LinkMatches($fixture.Link, $identity, $script:Binary) -and
        [ThreadlineToastShortcut]::LinkHasActivator($fixture.Link, $classId)
    Add-Evidence 'Verify the temporary shortcut target, app identity and activator match the Threadline fixture' $linkMatches
    if (!$linkMatches) { Stop-Blocked 'The isolated Start shortcut could not be verified against the exact Threadline registration.' }
    if (Get-Command Get-StartApps -ErrorAction SilentlyContinue) {
        $startIndexed = $false
        $indexing = [Diagnostics.Stopwatch]::StartNew()
        while (!$startIndexed -and $indexing.ElapsedMilliseconds -lt 4500) {
            $startIndexed = @(Get-StartApps | Where-Object { $_.AppID -ceq $identity }).Count -eq 1
            if (!$startIndexed) { Start-Sleep -Milliseconds 100 }
        }
        Add-Evidence 'Find the exact Threadline identity in the per-user Start index' ([ordered]@{ found = $startIndexed; elapsed_ms = $indexing.ElapsedMilliseconds })
    }
}

function Clear-AppToastFixtureHistory {
    $fixture = $script:toastFixture
    $output = Join-Path $ReportDir ('fixture-history-cleanup-' + [guid]::NewGuid().ToString('N') + '.json')
    $info = New-Object System.Diagnostics.ProcessStartInfo
    $info.FileName = $script:Binary
    $info.Arguments = '--cleanup-test-toast "' + $output + '"'
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.EnvironmentVariables['THREADLINE_TEST_TOAST_TOKEN'] = $fixture.Token
    $cleanup = $null
    $checked = $false
    $exitCode = $null
    $outcome = $null
    try {
        $cleanup = [Diagnostics.Process]::Start($info)
        if ($cleanup.WaitForExit(6500)) {
            $exitCode = $cleanup.ExitCode
            if ([IO.File]::Exists($output)) {
                $outcome = Get-Content -LiteralPath $output -Raw | ConvertFrom-Json
                $checked = $exitCode -eq 0 -and $outcome.verified_absent -eq $true
            }
        }
    } finally {
        if ($null -ne $cleanup) {
            if (!$cleanup.HasExited) { $cleanup.Kill(); $cleanup.WaitForExit() }
            $cleanup.Dispose()
        }
        Add-Evidence 'Clear only the owned app history and verify absence before deregistration' ([ordered]@{
            verified_absent = $checked; exit_code = $exitCode; native_result = $outcome
        })
    }
    return $checked
}

function Remove-AppToastFixture {
    $fixture = $script:toastFixture
    if ($null -eq $fixture) { return $true }
    $linkRemoved = $false
    try { $linkRemoved = [ThreadlineToastShortcut]::RemoveOwnedLink($fixture.Link, $fixture.Identity) }
    catch { }
    $classRemoved = !$fixture.ClassCreated
    if ($fixture.ClassCreated) {
        $classes = $null
        try {
            $classes = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Software\Classes\CLSID', $true)
            $entry = $classes.OpenSubKey($fixture.ClassName, $true)
            if ($null -eq $entry) { $classRemoved = $true }
            elseif ($entry.GetValue('ThreadlineFeasibilityOwner') -ceq $fixture.Token -or
                ($entry.ValueCount -eq 0 -and $entry.SubKeyCount -eq 0)) {
                try {
                    $server = $entry.OpenSubKey('LocalServer32')
                    $serverOwned = $false
                    if ($null -ne $server) {
                        try {
                            $serverOwned = $server.SubKeyCount -eq 0 -and ($server.ValueCount -eq 0 -or
                                ($server.ValueCount -eq 1 -and $server.GetValue('') -ceq $fixture.Command))
                        }
                        finally { $server.Dispose() }
                    }
                    if ($serverOwned -and $entry.SubKeyCount -eq 1 -and $entry.ValueCount -eq 1) {
                        $entry.DeleteSubKey('LocalServer32', $true)
                        $classRemoved = $true
                    }
                    if ($entry.SubKeyCount -eq 0 -and $entry.ValueCount -eq 1) { $classRemoved = $true }
                    if ($entry.SubKeyCount -eq 0 -and $entry.ValueCount -eq 0) { $classRemoved = $true }
                } finally { $entry.Dispose() }
                if ($classRemoved) { $classes.DeleteSubKey($fixture.ClassName, $true) }
            } else { $entry.Dispose() }
        } catch { $classRemoved = $false }
        finally { if ($null -ne $classes) { $classes.Dispose() } }
    }
    Add-Evidence 'Remove the verified test shortcut and its unchanged per-user COM registration' ([ordered]@{ shortcut = $linkRemoved; class_registration = $classRemoved })
    return $linkRemoved -and $classRemoved
}
