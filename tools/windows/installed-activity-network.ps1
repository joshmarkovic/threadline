function Start-ActivityFixtureProxy {
    $script:activityProxy = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
    $activityProxy.Start()
}
function Start-ActivityFixtureProcess {
    $endpoint = 'http://127.0.0.1:' + $activityProxy.LocalEndpoint.Port
    $info = New-Object Diagnostics.ProcessStartInfo
    $info.FileName = $exe
    $info.UseShellExecute = $false
    foreach ($name in @('HTTP_PROXY','HTTPS_PROXY','ALL_PROXY')) { $info.EnvironmentVariables[$name] = $endpoint }
    $info.EnvironmentVariables['NO_PROXY'] = ''
    return [Diagnostics.Process]::Start($info)
}
