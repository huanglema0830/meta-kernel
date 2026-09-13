' ============================================================
'  Cloud Kernel - hidden auto-probe runner
'  Waits for the network, then runs autoprobe.ps1 invisibly.
'  Used by the auto-start entry (runs at every logon).
' ============================================================
Option Explicit

Dim fso, sh, base, ps1
Set fso = CreateObject("Scripting.FileSystemObject")
Set sh  = CreateObject("WScript.Shell")

base = fso.GetParentFolderName(WScript.ScriptFullName)
ps1  = base & "\autoprobe.ps1"

If Not fso.FileExists(ps1) Then
    WScript.Quit 1
End If

' give the network a moment to come up after logon
WScript.Sleep 25000

' run hidden (0) and wait (True) so only one instance runs at a time
sh.Run "powershell.exe -NoProfile -ExecutionPolicy Bypass -File """ & ps1 & """", 0, True
