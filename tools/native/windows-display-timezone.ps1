function Wait-OwnedDisplayLabel([IntPtr]$Window, [string]$Id, [string]$Label) {
    Wait-For "the owned $Id label with the expected text" {
        $null -ne (Find-OwnedFixtureControl $Window $Id $Label -AllowDisabled)
    }
}

function Save-FixtureDisplayTimeZone([IntPtr]$Window, [ValidateSet('America/New_York', 'UTC')][string]$Zone) {
    Wait-For 'the owned display time zone field' {
        Set-OwnedFixtureEditValue $Window 'display-timezone' 'Display time zone' $Zone
    }
    Wait-For 'the new time zone in the owned edit field' {
        $edit = Get-OwnedFixtureEditPattern $Window 'display-timezone' 'Display time zone'
        $null -ne $edit -and $edit.Current.Value -ceq $Zone
    }
    Wait-For 'the owned Save time zone action' {
        Invoke-OwnedFixtureControl $Window 'display-timezone-save' $null 'Save time zone'
    }
    Wait-OwnedDisplayLabel $Window 'display-timezone-message' 'Display time zone saved.'
}

function Invoke-DisplayTimeZoneProof([IntPtr]$Window, [string]$Profile) {
    $dot = ' ' + [char]0x00b7 + ' '
    $timeLabel = 'reader-item-times-0'
    $zoneLabel = 'display-timezone-effective'
    $utc = 'fixture-author' + $dot + 'Created 2023-07-12 12:00 UTC (+00:00)' + $dot + 'Updated 2023-11-14 22:13 UTC (+00:00)'
    $local = 'fixture-author' + $dot + 'Created 2023-07-12 08:00 EDT (-04:00)' + $dot + 'Updated 2023-11-14 17:13 EST (-05:00)'
    $effectiveUtc = 'Display time zone' + $dot + 'UTC'
    $effectiveLocal = 'Display time zone' + $dot + 'America/New_York'
    $null = Wait-Reader 'the cached offline workspace' {
        param($reader) $reader.offline -and $reader.signed_out -and $reader.cached_content_ready -and $reader.pane_count -eq 2
    }
    Click-AccountControl $Window 'reader-tab-0-0'
    $null = Wait-Reader 'the original cached issue in its own pane' {
        param($reader) $reader.active_number -eq 1 -and $reader.focused_pane -eq 0 -and $reader.offline -and $reader.signed_out
    }
    Click-AccountControl $Window 'reader-maximize-pane'
    Wait-OwnedDisplayLabel $Window $timeLabel $utc
    Click-AccountControl $Window 'account-open'
    Wait-OwnedDisplayLabel $Window $zoneLabel $effectiveUtc
    Save-FixtureDisplayTimeZone $Window 'America/New_York'
    Wait-OwnedDisplayLabel $Window $zoneLabel $effectiveLocal
    Click-AccountControl $Window 'account-close'
    Wait-OwnedDisplayLabel $Window $timeLabel $local
    Add-Evidence 'Display one summer and one winter cached instant in the chosen time zone' ([ordered]@{
        issue_number = 1; time_zone = 'America/New_York'; before = $utc; after = $local
    })

    $window = Restart-Reader $Profile
    $null = Wait-Reader 'the same offline cached issue after a fresh launch' {
        param($reader) $reader.active_number -eq 1 -and $reader.focused_pane -eq 0 -and $reader.offline -and $reader.signed_out
    }
    Wait-OwnedDisplayLabel $window $timeLabel $local
    Click-AccountControl $window 'account-open'
    Wait-OwnedDisplayLabel $window $zoneLabel $effectiveLocal
    Save-FixtureDisplayTimeZone $window 'UTC'
    Wait-OwnedDisplayLabel $window $zoneLabel $effectiveUtc
    Click-AccountControl $window 'account-close'
    Wait-OwnedDisplayLabel $window $timeLabel $utc
    Add-Evidence 'Restore the chosen time zone across a process restart and change back to UTC' ([ordered]@{
        issue_number = 1; persisted = $local; changed_back = $utc
    })
    Stop-ThroughUi
    $script:report.detail = 'Owned account controls persisted the display time zone, showed the exact summer and winter offsets on a cached issue, and restored UTC after a separate offline restart.'
}
