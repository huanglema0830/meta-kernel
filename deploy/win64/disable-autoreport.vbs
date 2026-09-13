' ============================================================
'  Disable AUTO-REPORT on this machine.
'  Removes the logon entry and stops any running hidden probe.
' ============================================================
Option Explicit

Dim fso, sh, wmi, col, p, killed
Set fso = CreateObject("Scripting.FileSystemObject")
Set sh  = CreateObject("WScript.Shell")
Set wmi = GetObject("winmgmts:\\.\root\cimv2")

On Error Resume Next
sh.RegDelete "HKCU\Software\Microsoft\Windows\CurrentVersion\Run\CloudKernelAutoProbe"
On Error GoTo 0

killed = 0
Set col = wmi.ExecQuery("SELECT ProcessId, CommandLine FROM Win32_Process WHERE Name='wscript.exe'")
For Each p In col
    If Not IsNull(p.CommandLine) Then
        If InStr(LCase(p.CommandLine), "autoprobe-hidden.vbs") > 0 Then
            On Error Resume Next
            p.Terminate()
            On Error GoTo 0
            killed = killed + 1
        End If
    End If
Next

WScript.Echo "OK - auto-report disabled on this machine." & vbCrLf & _
             "Stopped hidden probe instances: " & killed
