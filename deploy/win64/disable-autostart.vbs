' ============================================================
'  Disable auto-start + stop everything
'   - removes HKCU Run entry
'   - terminates the watchdog
'   - terminates the gateway
' ============================================================
Option Explicit

Dim fso, sh, wmi, col, p, col2, q
Set fso = CreateObject("Scripting.FileSystemObject")
Set sh  = CreateObject("WScript.Shell")
Set wmi = GetObject("winmgmts:\\.\root\cimv2")

' ---- remove autostart entry ----
On Error Resume Next
sh.RegDelete "HKCU\Software\Microsoft\Windows\CurrentVersion\Run\CloudKernelGateway"
On Error GoTo 0

' ---- terminate watchdog ----
Dim killedWd
killedWd = 0
Set col = wmi.ExecQuery("SELECT ProcessId, CommandLine FROM Win32_Process WHERE Name='wscript.exe'")
For Each p In col
    If Not IsNull(p.CommandLine) Then
        If InStr(LCase(p.CommandLine), "watchdog.vbs") > 0 Then
            On Error Resume Next
            p.Terminate()
            On Error GoTo 0
            killedWd = killedWd + 1
        End If
    End If
Next

' ---- terminate gateway ----
Dim killedGw
killedGw = 0
Set col2 = wmi.ExecQuery("SELECT ProcessId FROM Win32_Process WHERE Name='npb-gateway.exe'")
For Each q In col2
    On Error Resume Next
    q.Terminate()
    On Error GoTo 0
    killedGw = killedGw + 1
Next

WScript.Echo "OK - stopped." & vbCrLf & vbCrLf & _
             "Auto-start entry removed." & vbCrLf & _
             "Watchdog terminated: " & killedWd & vbCrLf & _
             "Gateway terminated: " & killedGw
